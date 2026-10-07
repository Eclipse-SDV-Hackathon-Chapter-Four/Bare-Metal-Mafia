#!/usr/bin/env python3
# Copyright (c) 2026 Contributors to the Bare-Metal-Mafia project
# See the NOTICE file(s) distributed with this work for additional
# information regarding copyright ownership.
#
# This program and the accompanying materials are made available under the
# terms of the Apache License, Version 2.0 which is available at
# https://www.apache.org/licenses/LICENSE-2.0
#
# AI Disclosure: This file was largely AI-generated. The AI-generated
# portions are made available under CC0-1.0 and not subject to the
# project's licence. The human contributor has reviewed and verified
# that the code is correct.
#
# SPDX-License-Identifier: Apache-2.0 AND CC0-1.0
# Assisted-by: Anthropic Claude Opus 5.5 (claude-opus-5-5)

"""ROS 2 side checks for the Gazebo cabin, used by gazebo-sim/test-window.sh.

Runs inside the gazebo-sim container (same ROS_DOMAIN_ID as the bridge).
Every subcommand prints one result line and exits 0 on success, 1 on failure.

  sim_check.py window --percent 25 [--tolerance 0.005] [--timeout 20]
      publish the setpoint for <percent> on /sim/window/row2_left/position_cmd
      and wait until /sim/joint_states reports the joint within tolerance
  sim_check.py joint --percent 25 [--tolerance 0.005] [--timeout 60]
      read-only: publish nothing, wait until /sim/joint_states reports the
      window joint at <percent> (someone else, e.g. ros-up-bridge, moves it)
  sim_check.py contact --expect present|absent [--timeout 5]
      present: a /sim/seat/row2/contact message naming child_seat arrives
      absent:  no /sim/seat/row2/contact message arrives within the timeout
               (Gazebo publishes contacts only while something touches)
  sim_check.py clock [--timeout 10]
      /clock is bridged and simulation time advances
"""

import argparse
import os
import sys
import time

import rclpy
import yaml
from rclpy.node import Node
from rosgraph_msgs.msg import Clock
from ros_gz_interfaces.msg import Contacts
from sensor_msgs.msg import JointState
from std_msgs.msg import Float64

SHARE = os.environ.get('GAZEBO_SIM_SHARE', '/opt/gazebo_sim')
CMD_TOPIC = '/sim/window/row2_left/position_cmd'
JOINT_STATES_TOPIC = '/sim/joint_states'
CONTACT_TOPIC = '/sim/seat/row2/contact'
CHILD_SEAT_MODEL = 'child_seat'


def percent_to_position(percent):
    """Apply the mapping defined in config/window.yaml."""
    with open(os.path.join(SHARE, 'config', 'window.yaml')) as f:
        window = yaml.safe_load(f)['window_row2_left']
    position = window['closed_position_m'] + (percent / 100.0) * window['travel_m']
    return window['joint_name'], position


def spin_until(node, done, timeout_s):
    deadline = time.monotonic() + timeout_s
    while time.monotonic() < deadline:
        rclpy.spin_once(node, timeout_sec=0.1)
        if done():
            return True
    return False


def watch_joint(node, args, publish):
    """Wait until the window joint is within tolerance of args.percent.

    publish=True drives the joint there itself; publish=False only observes.
    """
    if not 0.0 <= args.percent <= 100.0:
        print(f'invalid percent {args.percent}')
        return False
    joint, target = percent_to_position(args.percent)
    state = {'position': None, 'in_tol_since': None}

    def on_joint_states(msg):
        if joint not in msg.name:
            return
        pos = msg.position[msg.name.index(joint)]
        state['position'] = pos
        if abs(pos - target) <= args.tolerance:
            state['in_tol_since'] = state['in_tol_since'] or time.monotonic()
        else:
            state['in_tol_since'] = None

    node.create_subscription(JointState, JOINT_STATES_TOPIC, on_joint_states, 10)
    if publish:
        pub = node.create_publisher(Float64, CMD_TOPIC, 10)
        # Re-publish the setpoint until done: the first messages can be lost
        # while DDS discovery between this node and the bridge completes.
        node.create_timer(0.2, lambda: pub.publish(Float64(data=target)))

    settled = spin_until(
        node,
        lambda: state['in_tol_since'] is not None
        and time.monotonic() - state['in_tol_since'] >= args.settle,
        args.timeout,
    )
    pos = state['position']
    pos_txt = 'no joint_states received' if pos is None else f'{pos:.4f} m'
    verb = 'window' if publish else 'joint (observed)'
    print(f'{verb} {args.percent:g}% -> target {target:.4f} m, '
          f'actual {pos_txt}, tolerance {args.tolerance:.4f} m')
    return settled


def check_window(node, args):
    return watch_joint(node, args, publish=True)


def check_joint(node, args):
    return watch_joint(node, args, publish=False)


def check_contact(node, args):
    seen = {'any': 0, 'child_seat': 0}

    def on_contacts(msg):
        seen['any'] += 1
        for c in msg.contacts:
            names = (c.collision1.name, c.collision2.name)
            if any(n.split('::')[0] == CHILD_SEAT_MODEL for n in names):
                seen['child_seat'] += 1
                return

    node.create_subscription(Contacts, CONTACT_TOPIC, on_contacts, 10)
    # Silence only means "absent" once the bridge's publisher is discovered;
    # before that, missing messages would prove nothing.
    if not spin_until(node, lambda: node.count_publishers(CONTACT_TOPIC) > 0, 10.0):
        print(f'contact: no publisher on {CONTACT_TOPIC} (bridge not running?)')
        return False
    if args.expect == 'present':
        ok = spin_until(node, lambda: seen['child_seat'] > 0, args.timeout)
        print(f'contact present: {seen["child_seat"]} message(s) naming '
              f'{CHILD_SEAT_MODEL} within {args.timeout:g} s')
        return ok
    spin_until(node, lambda: seen['any'] > 0, args.timeout)
    print(f'contact absent: {seen["any"]} message(s) within {args.timeout:g} s')
    return seen['any'] == 0


def check_clock(node, args):
    stamps = []

    def on_clock(msg):
        stamps.append(msg.clock.sec + msg.clock.nanosec * 1e-9)

    node.create_subscription(Clock, '/clock', on_clock, 10)
    first = spin_until(node, lambda: len(stamps) > 0, args.timeout)
    if first:
        spin_until(node, lambda: stamps[-1] - stamps[0] >= 0.5, args.timeout)
    if not stamps:
        print('clock: no /clock message received')
        return False
    print(f'clock: sim time {stamps[0]:.3f} s -> {stamps[-1]:.3f} s '
          f'over {len(stamps)} message(s)')
    return stamps[-1] - stamps[0] >= 0.5


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest='cmd', required=True)
    p = sub.add_parser('window')
    p.add_argument('--percent', type=float, required=True)
    p.add_argument('--tolerance', type=float, default=0.005)
    p.add_argument('--settle', type=float, default=0.5,
                   help='seconds the joint must stay within tolerance')
    p.add_argument('--timeout', type=float, default=20.0)
    p = sub.add_parser('joint')
    p.add_argument('--percent', type=float, required=True)
    p.add_argument('--tolerance', type=float, default=0.005)
    p.add_argument('--settle', type=float, default=0.5,
                   help='seconds the joint must stay within tolerance')
    p.add_argument('--timeout', type=float, default=60.0)
    p = sub.add_parser('contact')
    p.add_argument('--expect', choices=('present', 'absent'), required=True)
    p.add_argument('--timeout', type=float, default=5.0)
    p = sub.add_parser('clock')
    p.add_argument('--timeout', type=float, default=10.0)
    args = parser.parse_args()

    rclpy.init()
    node = Node('gazebo_sim_check')
    try:
        ok = {'window': check_window, 'joint': check_joint, 'contact': check_contact,
              'clock': check_clock}[args.cmd](node, args)
    finally:
        node.destroy_node()
        rclpy.shutdown()
    sys.exit(0 if ok else 1)


if __name__ == '__main__':
    main()
