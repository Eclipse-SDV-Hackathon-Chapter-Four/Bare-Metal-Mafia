# ROS2 HVAC Host CAN Integration Guide

This document explains how to use host CAN (vcan0) with the `ros2-hvac` service to:

- set HVAC parameters from CAN frames (host -> ROS 2 HVAC)
- receive HVAC state frames from ROS 2 HVAC (ROS 2 HVAC -> host)

## 1. Prerequisites

- Linux host (or Linux VM) with SocketCAN support
- `vcan` kernel module loaded
- `can-utils` installed (`cansend`, `candump`)
- `podman-compose` stack from this repository

The repository `Vagrantfile` already provisions `vcan` and `can_gw` for the openDuT scenario.

## 2. Host vcan setup

Run on the Linux host:

```bash
sudo modprobe vcan
sudo ip link add dev vcan0 type vcan 2>/dev/null || true
sudo ip link set vcan0 up
ip -details link show vcan0
```

## 3. Compose configuration

In `docker-compose.yml`, the `ros2-hvac` service has these CAN environment variables:

- `CAN_ENABLED` (default `"false"`)
- `CAN_BUSTYPE` (default `socketcan`)
- `CAN_CHANNEL` (default `vcan0`)
- `CAN_TX_PERIOD_S` (default `"1.0"`)
- `CAN_TX_ID` (default `"0x320"`)
- `CAN_RX_ID` (default `"0x321"`)
- `CAN_BRINGUP_VCAN` (default `"false"`)

Recommended for host-managed CAN:

- set `CAN_ENABLED: "true"`
- keep `CAN_BRINGUP_VCAN: "false"`
- keep `CAN_CHANNEL: vcan0`

## 4. Start the stack

```bash
podman-compose --profile ros2 -f docker-compose.yml up --build
```

If you changed files under `ros2-hvac/ros2_ws/src/hack_to_the_future_hvac`, refresh the stack artifact:

```bash
./ros2-hvac/build-hvac-artifact.sh
```

Then update the checksum in `ros2-hvac/runtime/hvac_stack_archive.json` to match the new tarball hash.

## 5. CAN frame contract

The HVAC simulator uses 11-bit standard CAN frames with 3-byte payloads.

- RX frame ID: `CAN_RX_ID` (default `0x321`) -> updates ROS 2 parameters
- TX frame ID: `CAN_TX_ID` (default `0x320`) -> publishes current HVAC state

Payload bytes:

- byte 0: `target_temperature_celsius` (clamped 16..30)
- byte 1: `fan_speed_percent` (clamped 0..100)
- byte 2: flags bitfield
  - bit 0 (`0x01`): `air_conditioning_active`
  - bit 1 (`0x02`): `fault_active`

## 6. Send parameters from host to ROS2 HVAC

Example: target=22 C, fan=10%, AC=true, fault=false

```bash
cansend vcan0 321#160A01
```

Another example: target=24 C, fan=40%, AC=true, fault=true

```bash
cansend vcan0 321#182803
```

Verify parameter updates inside container:

```bash
podman-compose --profile ros2 -f docker-compose.yml exec ros2-hvac \
  ros2 param get /hvac_simulator target_temperature_celsius
podman-compose --profile ros2 -f docker-compose.yml exec ros2-hvac \
  ros2 param get /hvac_simulator fan_speed_percent
podman-compose --profile ros2 -f docker-compose.yml exec ros2-hvac \
  ros2 param get /hvac_simulator air_conditioning_active
podman-compose --profile ros2 -f docker-compose.yml exec ros2-hvac \
  ros2 param get /hvac_simulator fault_active
```

## 7. Receive state signals from ROS2 HVAC on host

Listen for outgoing HVAC state frames:

```bash
candump vcan0
```

You should see frames with ID `320` (hex) at roughly `CAN_TX_PERIOD_S` interval.

## 8. openDuT / EDGAR usage model

- Step 1 (single-host test): ROBOT + testee use host networking and host `vcan0`.
- Step 2 (distributed): each side uses local `vcan0`; EDGAR forwards CAN frames between hosts.

No simulator code changes are needed between step 1 and step 2 if CAN IDs/channel stay consistent.

## 9. Troubleshooting

- `No such device` for `vcan0`: create and bring up `vcan0` on host.
- `Operation not permitted` on CAN socket: ensure container has needed capabilities and host supports CAN access for container runtime.
- No RX updates in ROS params: verify `CAN_ENABLED=true`, correct `CAN_RX_ID`, and host is sending to the same channel.
- No TX frames in `candump`: verify simulator is running and `CAN_ENABLED=true`.
