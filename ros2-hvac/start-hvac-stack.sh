#!/usr/bin/env bash
set -eo pipefail

source /opt/ros/${ROS_DISTRO}/setup.bash
source /opt/muto_ws/install/setup.bash
source /opt/hvac_ws/install/setup.bash

mkdir -p "${HOME}/.ros2_medkit"

ros2 launch hack_to_the_future_hvac muto.launch.py \
  vehicle_namespace:="${MUTO_VEHICLE_NAMESPACE:-org.eclipse.muto.guardian}" \
  vehicle_name:="${MUTO_VEHICLE_NAME:-guardian-hvac}" &
MUTO_PID=$!

sleep "${MUTO_BOOTSTRAP_DELAY_S:-8}"

ros2 run hack_to_the_future_hvac deploy_stack \
  --ros-args \
  -p stack_path:="/opt/hvac_ws/install/share/hack_to_the_future_hvac/config/hvac_stack.json" &
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
