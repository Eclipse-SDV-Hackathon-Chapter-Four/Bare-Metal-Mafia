#!/usr/bin/env bash
set -eo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_DIR="${SCRIPT_DIR}"
LOG_FILE="${WORKSPACE_DIR}/run.log"

exec > >(tee -a "${LOG_FILE}") 2>&1

echo "[artifact-run] starting at $(date -Iseconds)"
echo "[artifact-run] workspace=${WORKSPACE_DIR}"
echo "[artifact-run] ros_distro=${ROS_DISTRO:-humble}"

cd "${WORKSPACE_DIR}"

source /opt/ros/${ROS_DISTRO:-humble}/setup.bash
echo "[artifact-run] sourced /opt/ros/${ROS_DISTRO:-humble}/setup.bash"

if [ -f "${WORKSPACE_DIR}/install/setup.bash" ]; then
  source "${WORKSPACE_DIR}/install/setup.bash"
  echo "[artifact-run] sourced ${WORKSPACE_DIR}/install/setup.bash"
else
  echo "[artifact-run] missing ${WORKSPACE_DIR}/install/setup.bash"
fi

echo "[artifact-run] ros2 pkg prefix hack_to_the_future_hvac"
ros2 pkg prefix hack_to_the_future_hvac
echo "[artifact-run] launching hvac.launch.py"
exec ros2 launch hack_to_the_future_hvac hvac.launch.py
