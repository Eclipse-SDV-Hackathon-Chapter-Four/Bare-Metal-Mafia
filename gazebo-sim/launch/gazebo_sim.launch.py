"""Start the Gazebo Fortress cabin world and the ROS 2 <-> Gazebo bridge.

    ros2 launch /opt/gazebo_sim/launch/gazebo_sim.launch.py [gui:=true]

1. Reads config/window.yaml and writes travel_m into the window joint limit
   of worlds/cabin.sdf (rendered to /tmp/gazebo_sim/cabin.sdf).
2. Starts `ign gazebo -s -r` (server only, headless) or, with gui:=true,
   `ign gazebo -r` (server and GUI client in one process).
3. Starts ros_gz_bridge's parameter_bridge with config/bridge.yaml.

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
    gz_cmd = ['ign', 'gazebo', '-r', '-v', verbosity, world]
    if not gui:
        gz_cmd.insert(2, '-s')

    gazebo = ExecuteProcess(cmd=gz_cmd, name='gazebo', output='screen')
    bridge = Node(
        package='ros_gz_bridge',
        executable='parameter_bridge',
        name='gz_bridge',
        output='screen',
        parameters=[{'config_file': os.path.join(share, 'config', 'bridge.yaml')}],
    )

    shutdown_on_exit = [
        RegisterEventHandler(OnProcessExit(
            target_action=action,
            on_exit=[EmitEvent(event=Shutdown(reason=f'{name} exited'))],
        ))
        for action, name in ((gazebo, 'gazebo'), (bridge, 'gz_bridge'))
    ]
    return [gazebo, bridge, *shutdown_on_exit]


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
