from launch import LaunchDescription
from launch_ros.actions import Node


def generate_launch_description():
    return LaunchDescription(
        [
            Node(
                package="hack_to_the_future_hvac",
                executable="hvac_simulator",
                name="hvac_simulator",
                output="screen",
                parameters=[
                    {
                        "publish_interval_s": 2.0,
                        "target_temperature_celsius": 22,
                        "air_conditioning_active": False,
                        "fan_speed_percent": 0,
                        "fault_active": False,
                    }
                ],
            )
        ]
    )
