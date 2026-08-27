#!/usr/bin/env python3
import json
from pathlib import Path

from muto_msgs.msg import MutoAction
import rclpy
from rclpy.node import Node


class StackDeployer(Node):
    def __init__(self) -> None:
        super().__init__("hvac_stack_deployer")
        self.declare_parameter(
            "stack_path",
            "/opt/muto_runtime/hvac_stack_archive.json",
        )
        self.declare_parameter("stack_topic", "/muto/stack")
        self.declare_parameter("discovery_wait_s", 3)
        self.declare_parameter("shutdown_delay_s", 1)
        self.publisher = self.create_publisher(
            MutoAction, str(self.get_parameter("stack_topic").value), 10
        )
        self.published = False
        self.timer = self.create_timer(
            float(self.get_parameter("discovery_wait_s").value), self._publish_stack
        )

    def _publish_stack(self) -> None:
        if self.published:
            return

        self.timer.cancel()
        stack_path = Path(str(self.get_parameter("stack_path").value))
        payload = json.loads(stack_path.read_text(encoding="utf-8"))

        msg = MutoAction()
        msg.method = "start"
        msg.payload = json.dumps(payload)

        self.publisher.publish(msg)
        self.published = True
        self.get_logger().info(f"Published Muto stack from {stack_path}")
        self.shutdown_timer = self.create_timer(
            float(self.get_parameter("shutdown_delay_s").value), self._shutdown
        )

    def _shutdown(self) -> None:
        self.shutdown_timer.cancel()
        rclpy.shutdown()


def main() -> None:
    rclpy.init()
    node = StackDeployer()
    try:
        rclpy.spin(node)
    finally:
        if rclpy.ok():
            rclpy.shutdown()
        node.destroy_node()


if __name__ == "__main__":
    main()
