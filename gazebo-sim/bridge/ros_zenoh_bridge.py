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

"""ROS side of the Gazebo bridge (runs inside the gazebo-sim container).

Moves three things between ROS 2 and plain Zenoh keys, as small JSON:

    gazebo/window/cmd       {"percent": 25.0}  -> /sim/window/row2_left/position_cmd (metres)
    /sim/joint_states       -> gazebo/window/position {"percent": 24.6}   at most 10 Hz
    /sim/seat/row2/contact  -> gazebo/seat/contact    {"contacts": 3}     at most 20 Hz

Percent <-> metres uses config/window.yaml, the only place that mapping
lives. The Rust service gazebo_bridge maps these keys to uProtocol.

Environment: ZENOH_CONNECT (e.g. tcp/zenohd:7447), GAZEBO_SIM_SHARE.
"""

import json
import os
import threading

import rclpy
import yaml
import zenoh
from rclpy.node import Node
from ros_gz_interfaces.msg import Contacts
from sensor_msgs.msg import JointState
from std_msgs.msg import Float64

SHARE = os.environ.get('GAZEBO_SIM_SHARE', '/opt/gazebo_sim')
KEY_WINDOW_CMD = 'gazebo/window/cmd'
KEY_WINDOW_POSITION = 'gazebo/window/position'
KEY_SEAT_CONTACT = 'gazebo/seat/contact'


def load_window():
    with open(os.path.join(SHARE, 'config', 'window.yaml')) as f:
        w = yaml.safe_load(f)['window_row2_left']
    return w['joint_name'], float(w['closed_position_m']), float(w['travel_m'])


def open_zenoh():
    # Client mode like the Rust services (lib.rs open_up_transport): no own
    # listener, which would fail on hosts with IPv6 disabled. timeout -1 makes
    # open() wait until zenohd is reachable instead of giving up.
    config = zenoh.Config()
    config.insert_json5('mode', '"client"')
    config.insert_json5('connect/endpoints',
                        json.dumps([os.environ.get('ZENOH_CONNECT', 'tcp/zenohd:7447')]))
    config.insert_json5('connect/timeout_ms', '-1')
    return zenoh.open(config)


class Throttled:
    """Forwards the latest value at most every `period` seconds."""

    def __init__(self, node, period, send):
        self._lock = threading.Lock()
        self._pending = None
        self._send = send
        node.create_timer(period, self._flush)

    def offer(self, payload):
        with self._lock:
            self._pending = payload

    def _flush(self):
        with self._lock:
            payload, self._pending = self._pending, None
        if payload is not None:
            self._send(payload)


class GazeboBridge(Node):

    def __init__(self, session):
        super().__init__('gazebo_zenoh_bridge')
        self.joint, self.closed_m, self.travel_m = load_window()
        self.session = session

        self.cmd_pub = self.create_publisher(Float64, '/sim/window/row2_left/position_cmd', 10)
        self.cmd_sub = session.declare_subscriber(KEY_WINDOW_CMD, self.on_window_cmd)

        self.position = Throttled(self, 0.1, lambda p: session.put(KEY_WINDOW_POSITION, p))
        self.contact = Throttled(self, 0.05, lambda p: session.put(KEY_SEAT_CONTACT, p))
        self.create_subscription(JointState, '/sim/joint_states', self.on_joint_states, 10)
        self.create_subscription(Contacts, '/sim/seat/row2/contact', self.on_contact, 10)
        self.get_logger().info(
            f'joint {self.joint}, travel {self.travel_m} m, '
            f"zenoh {os.environ.get('ZENOH_CONNECT', 'tcp/zenohd:7447')}")

    def on_window_cmd(self, sample):
        try:
            percent = float(json.loads(sample.payload.to_bytes())['percent'])
        except (ValueError, KeyError, TypeError) as err:
            self.get_logger().warning(f'invalid window command: {err}')
            return
        percent = min(max(percent, 0.0), 100.0)
        self.cmd_pub.publish(Float64(data=self.closed_m + percent / 100.0 * self.travel_m))

    def on_joint_states(self, msg):
        if self.joint not in msg.name:
            return
        metres = msg.position[msg.name.index(self.joint)]
        percent = (metres - self.closed_m) / self.travel_m * 100.0
        self.position.offer(json.dumps({'percent': round(percent, 2)}))

    def on_contact(self, msg):
        self.contact.offer(json.dumps({'contacts': len(msg.contacts)}))


def main():
    rclpy.init()
    session = open_zenoh()
    node = GazeboBridge(session)
    try:
        rclpy.spin(node)
    finally:
        session.close()
        node.destroy_node()
        rclpy.shutdown()


if __name__ == '__main__':
    main()
