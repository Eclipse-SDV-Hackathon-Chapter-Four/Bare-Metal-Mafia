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

"""Start the Gazebo Fortress cabin world and the ROS 2 <-> Gazebo bridge.

    ros2 launch /opt/gazebo_sim/launch/gazebo_sim.launch.py [gui:=true]

1. Reads config/window.yaml and writes travel_m into the window joint limit
   of worlds/cabin.sdf (rendered to /tmp/gazebo_sim/cabin.sdf).
2. Starts the Gazebo server `ign gazebo -s -r` and, with gui:=true, the GUI
   client `ign gazebo -g --gui-config config/gui.config` as a second process.
   Server and GUI are deliberately not started as one `ign gazebo -r`: that
   lets the ign Ruby wrapper fork both from one already multi-threaded
   process, and the forked server intermittently hung before loading the
   world. Closing the GUI window still stops the container.
3. Starts ros_gz_bridge's parameter_bridge with config/bridge.yaml.
4. Only if ROS_UP_BRIDGE_MODE is mirror or replace: starts the ROS side of
   ros-up-bridge (ros_zenoh_bridge.py) with ROS_UP_BRIDGE_CONFIG. Without
   the variable the container behaves exactly as before.

If either process exits, the whole launch shuts down so the container stops
instead of running half a simulation.
"""

import os

import yaml
from launch import LaunchDescription
from launch.actions import (DeclareLaunchArgument, EmitEvent, ExecuteProcess,
                            OpaqueFunction, RegisterEventHandler)
from launch.event_handlers import OnProcessExit
from launch.events import Shutdown
from launch.substitutions import LaunchConfiguration
from launch_ros.actions import Node

DEFAULT_SHARE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
RENDER_DIR = '/tmp/gazebo_sim'
TRAVEL_PLACEHOLDER = '@WINDOW_TRAVEL_M@'
ROS_UP_BRIDGE_SCRIPT = '/opt/ros_up_bridge/ros_side/ros_zenoh_bridge.py'


def _truthy(value):
    return value.strip().lower() in ('1', 'true', 'yes', 'on')


def _render_world(share):
    with open(os.path.join(share, 'config', 'window.yaml')) as f:
        window = yaml.safe_load(f)['window_row2_left']
    travel_m = float(window['travel_m'])
    if not 0.0 < travel_m <= 0.40:
        raise RuntimeError(f'window travel_m={travel_m} outside (0, 0.40]')

    with open(os.path.join(share, 'worlds', 'cabin.sdf')) as f:
        world = f.read()
    if TRAVEL_PLACEHOLDER not in world:
        raise RuntimeError(f'{TRAVEL_PLACEHOLDER} missing from cabin.sdf')

    os.makedirs(RENDER_DIR, exist_ok=True)
    path = os.path.join(RENDER_DIR, 'cabin.sdf')
    with open(path, 'w') as f:
        f.write(world.replace(TRAVEL_PLACEHOLDER, f'{travel_m:.6f}'))
    return path


def _launch_setup(context):
    share = LaunchConfiguration('share').perform(context)
    gui = _truthy(LaunchConfiguration('gui').perform(context))
    verbosity = LaunchConfiguration('verbosity').perform(context)

    world = _render_world(share)
    gazebo = ExecuteProcess(cmd=['ign', 'gazebo', '-s', '-r', '-v', verbosity, world],
                            name='gazebo', output='screen')
    bridge = Node(
        package='ros_gz_bridge',
        executable='parameter_bridge',
        name='gz_bridge',
        output='screen',
        parameters=[{'config_file': os.path.join(share, 'config', 'bridge.yaml')}],
    )

    processes = [(gazebo, 'gazebo'), (bridge, 'gz_bridge')]
    if gui:
        # Start camera aimed at the rear-left door and seat (config/gui.config).
        processes.append((ExecuteProcess(
            cmd=['ign', 'gazebo', '-g', '-v', verbosity,
                 '--gui-config', os.path.join(share, 'config', 'gui.config')],
            name='gazebo_gui', output='screen'), 'gazebo_gui'))

    mode = os.environ.get('ROS_UP_BRIDGE_MODE', '').strip().lower()
    if mode:
        if mode not in ('mirror', 'replace'):
            raise RuntimeError(f'ROS_UP_BRIDGE_MODE={mode}: expected mirror or replace')
        mapping = os.environ.get('ROS_UP_BRIDGE_CONFIG',
                                 f'/opt/ros_up_bridge/config/{mode}.yaml')
        processes.append((ExecuteProcess(
            cmd=['python3', ROS_UP_BRIDGE_SCRIPT, '--config', mapping],
            name='ros_zenoh_bridge', output='screen'), 'ros_zenoh_bridge'))

    shutdown_on_exit = [
        RegisterEventHandler(OnProcessExit(
            target_action=action,
            on_exit=[EmitEvent(event=Shutdown(reason=f'{name} exited'))],
        ))
        for action, name in processes
    ]
    return [action for action, _ in processes] + shutdown_on_exit


def generate_launch_description():
    return LaunchDescription([
        DeclareLaunchArgument('share', default_value=DEFAULT_SHARE,
                              description='directory holding worlds/ and config/'),
        DeclareLaunchArgument('gui', default_value=os.environ.get('GZ_GUI', 'false'),
                              description='true: server + GUI, false: headless server'),
        DeclareLaunchArgument('verbosity', default_value=os.environ.get('GZ_VERBOSITY', '2'),
                              description='ign gazebo verbosity 0-4'),
        OpaqueFunction(function=_launch_setup),
    ])
