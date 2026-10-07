#!/usr/bin/env bash
# Smoke test for the Gazebo simulation backend (compose profile "gazebo").
#
# Exercises the running gazebo-sim container end to end, Gazebo world through
# ros_gz_bridge into ROS 2, and checks:
#
#   1. gazebo-sim container is running, on ROS_DOMAIN_ID 42
#   2. the Gazebo world is up with the explicit topic names from cabin.sdf
#   3. the bridged ROS 2 topics exist and /clock advances
#   4. window setpoints 25 % / 100 % / 0 % reach the expected joint position
#      on /sim/joint_states within tolerance (mapping: config/window.yaml)
#   5. seat contact sensor: silent when the seat is empty, reports child_seat
#      after child-seat.sh place, silent again after child-seat.sh remove
#
# Works on macOS / Windows (WSL2/git-bash) / Linux. Requires only a running
# stack:  podman-compose --profile gazebo -f docker-compose.yml up -d
#
# Usage:  ./gazebo-sim/test-window.sh
#   COMPOSE_CMD overrides the compose command (default: podman-compose;
#   use COMPOSE_CMD="docker compose" for Docker).
#   TOLERANCE_M overrides the joint position tolerance (default 0.005 m).
set -uo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE="${COMPOSE_CMD:-podman-compose}"
COMPOSE_FILE="${ROOT_DIR}/docker-compose.yml"
TOLERANCE_M="${TOLERANCE_M:-0.005}"

EXEC_SIM=(${COMPOSE} --profile gazebo -f "${COMPOSE_FILE}" exec gazebo-sim)
CHILD_SEAT="${ROOT_DIR}/gazebo-sim/child-seat.sh"

FAILURES=0

step() { printf '\n== %s ==\n' "$1"; }
pass() { printf 'PASS: %s\n' "$1"; }
fail() { printf 'FAIL: %s\n' "$1"; FAILURES=$((FAILURES + 1)); }

ros() {
  "${EXEC_SIM[@]}" bash -lc "source /opt/ros/humble/setup.bash >/dev/null 2>&1 && $*" 2>&1 | tr -d '\r'
}

# Runs tools/sim_check.py in the container; prints its result line and
# returns its exit code.
sim_check() {
  local out rc
  out="$("${EXEC_SIM[@]}" bash -lc "source /opt/ros/humble/setup.bash >/dev/null 2>&1 && python3 /opt/gazebo_sim/tools/sim_check.py $*" 2>&1)"
  rc=$?
  printf '  %s\n' "$(printf '%s' "${out}" | tr -d '\r' | tail -1)"
  return ${rc}
}

step "stack reachable"
if "${EXEC_SIM[@]}" true >/dev/null 2>&1; then
  pass "gazebo-sim container is running"
else
  fail "gazebo-sim container not running; start with: ${COMPOSE} --profile gazebo -f docker-compose.yml up -d"
  exit 1
fi
DOMAIN="$("${EXEC_SIM[@]}" printenv ROS_DOMAIN_ID 2>/dev/null | tr -d '\r')"
if [ "${DOMAIN}" = "42" ]; then
  pass "ROS_DOMAIN_ID=42 (isolated from ros2-hvac on 0)"
else
  fail "ROS_DOMAIN_ID is '${DOMAIN}', expected 42"
fi

step "Gazebo world up with explicit topics"
GZ_TOPICS=""
DEADLINE=$((SECONDS + 60))
while [ ${SECONDS} -lt ${DEADLINE} ]; do
  GZ_TOPICS="$("${EXEC_SIM[@]}" ign topic -l 2>/dev/null | tr -d '\r')"
  printf '%s\n' "${GZ_TOPICS}" | grep -qx "/model/cabin/joint_state" && break
  sleep 2
done
for t in /model/cabin/joint/window_row2_left_joint/cmd_pos /model/cabin/joint_state /clock; do
  if printf '%s\n' "${GZ_TOPICS}" | grep -qx "${t}"; then
    pass "gazebo topic ${t}"
  else
    fail "gazebo topic ${t} missing"
  fi
done

step "ROS 2 bridge topics"
ROS_TOPICS="$(ros "timeout 15 ros2 topic list")"
for t in /sim/window/row2_left/position_cmd /sim/joint_states /sim/seat/row2/contact /clock; do
  if printf '%s\n' "${ROS_TOPICS}" | grep -qx "${t}"; then
    pass "ros topic ${t}"
  else
    fail "ros topic ${t} missing"
  fi
done
if sim_check clock --timeout 10; then
  pass "/clock bridged and simulation time advancing"
else
  fail "/clock not advancing"
fi

step "window setpoints -> joint position (tolerance ${TOLERANCE_M} m)"
for pct in 25 100 0; do
  if sim_check window --percent "${pct}" --tolerance "${TOLERANCE_M}" --timeout 20; then
    pass "window reached ${pct} %"
  else
    fail "window did not reach ${pct} %"
  fi
done

step "seat contact sensor (row 2)"
"${CHILD_SEAT}" remove >/dev/null 2>&1
sleep 1
if sim_check contact --expect absent --timeout 3; then
  pass "empty seat: no contact messages"
else
  fail "empty seat still reports contacts"
fi
if "${CHILD_SEAT}" place && sim_check contact --expect present --timeout 5; then
  pass "child seat placed: contact reported"
else
  fail "child seat placed but no contact reported"
fi
if "${CHILD_SEAT}" remove && sleep 1 && sim_check contact --expect absent --timeout 3; then
  pass "child seat removed: contact messages stop"
else
  fail "child seat removed but contacts still reported"
fi

printf '\n'
if [ ${FAILURES} -eq 0 ]; then
  echo "ALL CHECKS PASSED"
else
  echo "${FAILURES} CHECK(S) FAILED"
  exit 1
fi
