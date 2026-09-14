#!/usr/bin/env bash
set -eo pipefail

source /opt/ros/${ROS_DISTRO}/setup.bash
source /opt/muto_ws/install/setup.bash

mkdir -p "${HOME}/.ros2_medkit"

CAN_CHANNEL="${CAN_CHANNEL:-vcan0}"
if [[ "${CAN_BRINGUP_VCAN:-false}" == "true" ]]; then
  modprobe vcan 2>/dev/null || true
  if ! ip link show "${CAN_CHANNEL}" >/dev/null 2>&1; then
    ip link add dev "${CAN_CHANNEL}" type vcan || true
  fi
  ip link set "${CAN_CHANNEL}" up || true
fi

ros2 launch /opt/muto_runtime/muto.launch.py \
  vehicle_namespace:="${MUTO_VEHICLE_NAMESPACE:-org.eclipse.muto.guardian}" \
  vehicle_name:="${MUTO_VEHICLE_NAME:-guardian-hvac}" &
MUTO_PID=$!

sleep "${MUTO_BOOTSTRAP_DELAY_S:-8}"

python3 /opt/muto_runtime/deploy_stack.py \
  --ros-args \
  -p stack_path:=/opt/muto_runtime/hvac_stack_archive.json \
  -p discovery_wait_s:="${MUTO_DEPLOY_DISCOVERY_WAIT_S:-3.0}" &
DEPLOY_PID=$!

ros2 launch ros2_medkit_gateway bringup.launch.py \
  enable_diagnostic_bridge:=true \
  server_host:=0.0.0.0 &
MEDKIT_PID=$!

cleanup() {
  kill "${DEPLOY_PID}" "${MEDKIT_PID}" "${MUTO_PID}" 2>/dev/null || true
}

trap cleanup EXIT INT TERM

exec /usr/local/bin/ros2_hvac_bridge
