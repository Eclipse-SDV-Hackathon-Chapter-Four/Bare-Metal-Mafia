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
            "/opt/hvac_ws/install/share/hack_to_the_future_hvac/config/hvac_stack.json",
        )
        self.declare_parameter("stack_topic", "/muto/stack")
        self.declare_parameter("context", "guardian.hvac")
        self.publisher = self.create_publisher(
            MutoAction, str(self.get_parameter("stack_topic").value), 10
        )
        self.timer = self.create_timer(2.0, self._publish_once)
        self.sent = False

    def _publish_once(self) -> None:
        if self.sent:
            return

        stack_path = Path(str(self.get_parameter("stack_path").value))
        payload = json.loads(stack_path.read_text(encoding="utf-8"))

        msg = MutoAction()
        msg.context = str(self.get_parameter("context").value)
        msg.method = "start"
        msg.payload = json.dumps(payload)

        self.publisher.publish(msg)
        self.get_logger().info(f"Published Muto stack from {stack_path}")
        self.sent = True
        self.timer.cancel()
        self.shutdown_timer = self.create_timer(0.5, self._shutdown)

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
