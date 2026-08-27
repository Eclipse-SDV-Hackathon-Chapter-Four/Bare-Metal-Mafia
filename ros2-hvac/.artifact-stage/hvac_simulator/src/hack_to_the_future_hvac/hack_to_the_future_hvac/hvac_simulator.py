from diagnostic_msgs.msg import DiagnosticArray, DiagnosticStatus, KeyValue
import rclpy
from rclpy.node import Node


class HvacSimulator(Node):
    def __init__(self) -> None:
        super().__init__("hvac_simulator")

        self.declare_parameter("publish_interval_s", 2.0)
        self.declare_parameter("target_temperature_celsius", 22)
        self.declare_parameter("air_conditioning_active", False)
        self.declare_parameter("fan_speed_percent", 0)
        self.declare_parameter("fault_active", False)

        self.diagnostic_publisher = self.create_publisher(DiagnosticArray, "/diagnostics", 10)

        interval_s = float(self.get_parameter("publish_interval_s").value)
        self.timer = self.create_timer(interval_s, self._tick)

    def _tick(self) -> None:
        target_temperature = int(self.get_parameter("target_temperature_celsius").value)
        hvac_active = bool(self.get_parameter("air_conditioning_active").value)
        fan_speed = int(self.get_parameter("fan_speed_percent").value)
        fault_active = bool(self.get_parameter("fault_active").value)

        diag_msg = DiagnosticArray()
        diag_msg.header.stamp = self.get_clock().now().to_msg()
        diag_msg.status.append(
            DiagnosticStatus(
                level=self._level_for_hvac(fault_active, hvac_active),
                name="guardian_hvac/thermal_state",
                message=self._message_for_hvac(fault_active, hvac_active),
                hardware_id="guardian-hvac-sim",
                values=[
                    KeyValue(key="target_temperature_celsius", value=str(target_temperature)),
                    KeyValue(key="air_conditioning_active", value=str(hvac_active).lower()),
                    KeyValue(key="fan_speed_percent", value=str(fan_speed)),
                    KeyValue(key="fault_active", value=str(fault_active).lower()),
                ],
            )
        )
        self.diagnostic_publisher.publish(diag_msg)

    def _level_for_hvac(self, fault_active: bool, hvac_active: bool) -> int:
        if fault_active:
            return DiagnosticStatus.ERROR
        if hvac_active:
            return DiagnosticStatus.WARN
        return DiagnosticStatus.OK

    def _message_for_hvac(self, fault_active: bool, hvac_active: bool) -> str:
        if fault_active:
            return "HVAC fault simulated"
        if hvac_active:
            return "HVAC cooling active"
        return "HVAC idle"


def main() -> None:
    rclpy.init()
    node = HvacSimulator()
    try:
        rclpy.spin(node)
    finally:
        node.destroy_node()
        rclpy.shutdown()
