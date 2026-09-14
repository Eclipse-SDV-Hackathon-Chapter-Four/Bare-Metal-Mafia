from diagnostic_msgs.msg import DiagnosticArray, DiagnosticStatus, KeyValue
import os
from typing import Optional

import rclpy
from rclpy.node import Node
from rclpy.parameter import Parameter

try:
    import can
except ImportError:
    can = None


class HvacSimulator(Node):
    def __init__(self) -> None:
        super().__init__("hvac_simulator")

        can_enabled_default = _env_bool("CAN_ENABLED", False)
        can_channel_default = os.getenv("CAN_CHANNEL", "vcan0")
        can_bustype_default = os.getenv("CAN_BUSTYPE", "socketcan")
        can_tx_period_default = _env_float("CAN_TX_PERIOD_S", 1.0)
        can_tx_id_default = _env_int("CAN_TX_ID", 0x320)
        can_rx_id_default = _env_int("CAN_RX_ID", 0x321)

        self.declare_parameter("publish_interval_s", 2.0)
        self.declare_parameter("target_temperature_celsius", 22)
        self.declare_parameter("air_conditioning_active", False)
        self.declare_parameter("fan_speed_percent", 0)
        self.declare_parameter("fault_active", False)
        self.declare_parameter("can_enabled", can_enabled_default)
        self.declare_parameter("can_channel", can_channel_default)
        self.declare_parameter("can_bustype", can_bustype_default)
        self.declare_parameter("can_tx_period_s", can_tx_period_default)
        self.declare_parameter("can_tx_id", can_tx_id_default)
        self.declare_parameter("can_rx_id", can_rx_id_default)

        self.diagnostic_publisher = self.create_publisher(DiagnosticArray, "/diagnostics", 10)
        self._can_bus: Optional["can.BusABC"] = None
        self._can_next_tx_time_s = 0.0
        self._can_init()

        interval_s = float(self.get_parameter("publish_interval_s").value)
        self.timer = self.create_timer(interval_s, self._tick)

    def _tick(self) -> None:
        self._can_receive_updates()

        target_temperature = int(self.get_parameter("target_temperature_celsius").value)
        hvac_active = bool(self.get_parameter("air_conditioning_active").value)
        fan_speed = int(self.get_parameter("fan_speed_percent").value)
        fault_active = bool(self.get_parameter("fault_active").value)

        self._can_publish_state(target_temperature, hvac_active, fan_speed, fault_active)

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

    def destroy_node(self):
        if self._can_bus is not None:
            try:
                self._can_bus.shutdown()
            except Exception:
                pass
        return super().destroy_node()

    def _can_init(self) -> None:
        can_enabled = bool(self.get_parameter("can_enabled").value)
        if not can_enabled:
            return
        if can is None:
            self.get_logger().warning("CAN enabled but python-can is not installed; CAN bridge disabled")
            return

        channel = str(self.get_parameter("can_channel").value)
        bustype = str(self.get_parameter("can_bustype").value)
        try:
            self._can_bus = can.Bus(interface=bustype, channel=channel)
            self.get_logger().info(f"CAN bridge enabled on {bustype}:{channel}")
        except Exception as exc:
            self.get_logger().error(f"Failed to initialize CAN bridge: {exc}")
            self._can_bus = None

    def _can_receive_updates(self) -> None:
        if self._can_bus is None:
            return
        rx_id = int(self.get_parameter("can_rx_id").value)

        try:
            while True:
                msg = self._can_bus.recv(timeout=0.0)
                if msg is None:
                    break
                if msg.arbitration_id != rx_id or len(msg.data) < 3:
                    continue

                target_temperature = max(16, min(30, int(msg.data[0])))
                fan_speed = max(0, min(100, int(msg.data[1])))
                flags = int(msg.data[2])
                hvac_active = bool(flags & 0x01)
                fault_active = bool(flags & 0x02)

                self.set_parameters(
                    [
                        Parameter("target_temperature_celsius", Parameter.Type.INTEGER, target_temperature),
                        Parameter("fan_speed_percent", Parameter.Type.INTEGER, fan_speed),
                        Parameter("air_conditioning_active", Parameter.Type.BOOL, hvac_active),
                        Parameter("fault_active", Parameter.Type.BOOL, fault_active),
                    ]
                )
        except Exception as exc:
            self.get_logger().error(f"CAN receive failed: {exc}")

    def _can_publish_state(
        self,
        target_temperature: int,
        hvac_active: bool,
        fan_speed: int,
        fault_active: bool,
    ) -> None:
        if self._can_bus is None:
            return

        now_s = self.get_clock().now().nanoseconds / 1_000_000_000.0
        tx_period_s = max(0.1, float(self.get_parameter("can_tx_period_s").value))
        if now_s < self._can_next_tx_time_s:
            return

        tx_id = int(self.get_parameter("can_tx_id").value)
        flags = (0x01 if hvac_active else 0x00) | (0x02 if fault_active else 0x00)
        payload = bytes(
            [
                max(16, min(30, int(target_temperature))),
                max(0, min(100, int(fan_speed))),
                flags,
            ]
        )

        try:
            self._can_bus.send(can.Message(arbitration_id=tx_id, data=payload, is_extended_id=False))
            self._can_next_tx_time_s = now_s + tx_period_s
        except Exception as exc:
            self.get_logger().error(f"CAN send failed: {exc}")


def _env_bool(key: str, default: bool) -> bool:
    value = os.getenv(key)
    if value is None:
        return default
    return value.strip().lower() in {"1", "true", "yes", "on"}


def _env_float(key: str, default: float) -> float:
    value = os.getenv(key)
    if value is None:
        return default
    try:
        return float(value)
    except ValueError:
        return default


def _env_int(key: str, default: int) -> int:
    value = os.getenv(key)
    if value is None:
        return default
    try:
        return int(value, 0)
    except ValueError:
        return default


def main() -> None:
    rclpy.init()
    node = HvacSimulator()
    try:
        rclpy.spin(node)
    finally:
        node.destroy_node()
        rclpy.shutdown()
