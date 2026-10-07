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
# Assisted-by: Anthropic Claude Fable 5.1 (claude-fable-5-1)

"""Generic ROS 2 <-> Zenoh JSON bridge: the ROS side of ros-up-bridge.

Runs inside the gazebo-sim container (started by the gazebo-sim launch file
when ROS_UP_BRIDGE_MODE is set). It reads the `links:` section of a
ros-up-bridge mapping file and, for every link, moves messages between one
ROS 2 topic and one plain Zenoh key:

    from_ros  ROS topic --message_to_ordereddict--> JSON --put--> <prefix>/<name>
    to_ros    <prefix>/<name> --JSON--> set_message_fields --publish--> ROS topic

Message types are resolved at runtime with rosidl_runtime_py.get_message, so
any installed ROS 2 message type works without code generation. uProtocol is
NOT spoken here; the Rust ros_up_mapper maps the Zenoh keys to uProtocol.

from_ros links may set max_rate_hz: the latest message is then forwarded at
most that often, and the last message of a burst is always forwarded.

    ros_zenoh_bridge.py --config /opt/ros_up_bridge/config/mirror.yaml

Environment: ZENOH_CONNECT (e.g. tcp/zenohd:7447), ROS_DOMAIN_ID.
"""

import argparse
import json
import os
import threading
import time

import rclpy
import yaml
import zenoh
from rclpy.node import Node
from rosidl_runtime_py.convert import message_to_ordereddict
from rosidl_runtime_py.set_message import set_message_fields
from rosidl_runtime_py.utilities import get_message


def open_zenoh():
    config = zenoh.Config()
    endpoint = os.environ.get('ZENOH_CONNECT')
    if endpoint:
        config.insert_json5('connect/endpoints', json.dumps([endpoint]))
    # Peer mode (the default) keeps retrying the endpoint in the background,
    # so the bridge may start before zenohd.
    return zenoh.open(config)


class FromRosLink:
    """ROS subscription -> JSON -> Zenoh put, optionally rate limited."""

    def __init__(self, node, session, key, cfg):
        self.node = node
        self.session = session
        self.key = key
        self.name = cfg['name']
        self.sent = 0
        self._lock = threading.Lock()
        self._pending = None
        msg_type = get_message(cfg['ros']['type'])
        node.create_subscription(msg_type, cfg['ros']['topic'], self._on_msg, 10)
        rate = cfg.get('max_rate_hz')
        self._throttled = bool(rate)
        if self._throttled:
            node.create_timer(1.0 / float(rate), self._flush)

    def _on_msg(self, msg):
        data = json.dumps(message_to_ordereddict(msg), default=str).encode()
        if not self._throttled:
            self._put(data)
            return
        with self._lock:
            self._pending = data

    def _flush(self):
        with self._lock:
            data, self._pending = self._pending, None
        if data is not None:
            self._put(data)

    def _put(self, data):
        self.session.put(self.key, data)
        self.sent += 1


class ToRosLink:
    """Zenoh subscriber -> JSON -> ROS message -> ROS publish."""

    def __init__(self, node, session, key, cfg):
        self.node = node
        self.name = cfg['name']
        self.received = 0
        self.msg_type = get_message(cfg['ros']['type'])
        self.pub = node.create_publisher(self.msg_type, cfg['ros']['topic'], 10)
        self.sub = session.declare_subscriber(key, self._on_sample)

    def _on_sample(self, sample):
        try:
            msg = self.msg_type()
            set_message_fields(msg, json.loads(sample.payload.to_bytes()))
        except Exception as err:  # bad payload must not kill the bridge
            self.node.get_logger().warning(f'{self.name}: dropped invalid payload: {err}')
            return
        self.pub.publish(msg)
        self.received += 1


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--config', required=True, help='ros-up-bridge mapping YAML')
    args = parser.parse_args()

    with open(args.config) as f:
        cfg = yaml.safe_load(f)
    prefix = cfg['zenoh']['key_prefix']

    rclpy.init()
    node = Node('ros_zenoh_bridge')
    session = open_zenoh()
    links = []
    for link in cfg.get('links', []):
        key = f"{prefix}/{link['name']}"
        cls = {'from_ros': FromRosLink, 'to_ros': ToRosLink}[link['direction']]
        links.append(cls(node, session, key, link))
        node.get_logger().info(
            f"link {link['name']}: {link['ros']['topic']} ({link['ros']['type']}) "
            f"{'->' if link['direction'] == 'from_ros' else '<-'} zenoh {key}")
    node.get_logger().info(f"mode {cfg.get('mode')}: {len(links)} link(s), "
                           f"zenoh connect {os.environ.get('ZENOH_CONNECT', '(scouting)')}")

    def report():
        while True:
            time.sleep(30)
            counts = ', '.join(
                f'{l.name}={getattr(l, "sent", getattr(l, "received", 0))}' for l in links)
            node.get_logger().info(f'messages forwarded: {counts}')

    threading.Thread(target=report, daemon=True).start()
    try:
        rclpy.spin(node)
    finally:
        session.close()
        node.destroy_node()
        rclpy.shutdown()


if __name__ == '__main__':
    main()
