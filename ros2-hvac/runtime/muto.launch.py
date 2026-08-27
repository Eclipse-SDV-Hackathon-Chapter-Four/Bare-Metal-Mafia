from launch import LaunchDescription
from launch.actions import DeclareLaunchArgument
from launch.substitutions import LaunchConfiguration
from launch_ros.actions import Node


def generate_launch_description():
    muto_namespace_arg = DeclareLaunchArgument("muto_namespace", default_value="muto")
    vehicle_namespace_arg = DeclareLaunchArgument(
        "vehicle_namespace",
        default_value="org.eclipse.muto.guardian",
        description="Vehicle ID namespace",
    )
    vehicle_name_arg = DeclareLaunchArgument(
        "vehicle_name",
        default_value="guardian-hvac",
        description="Vehicle name",
    )

    muto_params = "/opt/muto_runtime/muto.yaml"

    node_agent = Node(
        namespace=LaunchConfiguration("muto_namespace"),
        name="agent",
        package="muto_agent",
        executable="muto_agent",
        output="screen",
        parameters=[
            muto_params,
            {"namespace": LaunchConfiguration("vehicle_namespace")},
            {"name": LaunchConfiguration("vehicle_name")},
        ],
    )

    node_mqtt_gateway = Node(
        namespace=LaunchConfiguration("muto_namespace"),
        name="gateway",
        package="muto_agent",
        executable="mqtt",
        output="screen",
        parameters=[
            muto_params,
            {"namespace": LaunchConfiguration("vehicle_namespace")},
            {"name": LaunchConfiguration("vehicle_name")},
        ],
    )

    node_commands = Node(
        namespace=LaunchConfiguration("muto_namespace"),
        name="commands_plugin",
        package="muto_agent",
        executable="commands",
        output="screen",
        parameters=[
            muto_params,
            {"namespace": LaunchConfiguration("vehicle_namespace")},
            {"name": LaunchConfiguration("vehicle_name")},
        ],
    )

    node_twin = Node(
        namespace=LaunchConfiguration("muto_namespace"),
        name="core_twin",
        package="muto_core",
        executable="twin",
        output="screen",
        parameters=[
            muto_params,
            {"namespace": LaunchConfiguration("vehicle_namespace")},
            {"name": LaunchConfiguration("vehicle_name")},
        ],
    )

    composer_parameters = [
        muto_params,
        {"namespace": LaunchConfiguration("vehicle_namespace")},
        {"name": LaunchConfiguration("vehicle_name")},
        {"ignored_packages": [""]},
    ]

    node_composer = Node(
        namespace=LaunchConfiguration("muto_namespace"),
        name="muto_composer",
        package="muto_composer",
        executable="muto_composer",
        output="screen",
        parameters=composer_parameters,
    )

    node_compose_plugin = Node(
        namespace=LaunchConfiguration("muto_namespace"),
        name="compose_plugin",
        package="muto_composer",
        executable="compose_plugin",
        output="screen",
        parameters=composer_parameters,
    )

    node_provision_plugin = Node(
        namespace=LaunchConfiguration("muto_namespace"),
        name="provision_plugin",
        package="muto_composer",
        executable="provision_plugin",
        output="screen",
        parameters=composer_parameters,
    )

    node_launch_plugin = Node(
        namespace=LaunchConfiguration("muto_namespace"),
        name="launch_plugin",
        package="muto_composer",
        executable="launch_plugin",
        output="screen",
        parameters=composer_parameters,
    )

    ld = LaunchDescription()
    ld.add_action(muto_namespace_arg)
    ld.add_action(vehicle_namespace_arg)
    ld.add_action(vehicle_name_arg)
    ld.add_action(node_agent)
    ld.add_action(node_mqtt_gateway)
    ld.add_action(node_commands)
    ld.add_action(node_twin)
    ld.add_action(node_composer)
    ld.add_action(node_compose_plugin)
    ld.add_action(node_provision_plugin)
    ld.add_action(node_launch_plugin)
    return ld
