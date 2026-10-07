#!/usr/bin/env bash
# Copyright (c) 2026 Contributors to the Bare-Metal-Mafia project
# See the NOTICE file(s) distributed with this work for additional
# information regarding copyright ownership.
#
# This program and the accompanying materials are made available under the
# terms of the Apache License, Version 2.0 which is available at
# https://www.apache.org/licenses/LICENSE-2.0
#
# AI Disclosure: This file was largely AI-generated. The AI-generated
# portions are made available under CC0-1.0 and not subject to the
# project's licence. The human contributor has reviewed and verified
# that the code is correct.
#
# SPDX-License-Identifier: Apache-2.0 AND CC0-1.0
# Assisted-by: Anthropic Claude Opus 5.5 (claude-opus-5-5)
# Assisted-by: Anthropic Claude Fable 5.1 (claude-fable-5-1)

# End-to-end test for ros-up-bridge mode "mirror".
#
# (Re)starts the stack itself, in this order, because a mirror only sees
# window states published after it is up:
#   1. zenohd, gazebo-sim (bridge mode mirror) and ros-up-mapper
#   2. the rest of the default stack, whose scripted scenario escalates the
#      Guardian to MITIGATING with the window at 25 %
# and checks:
#   - static single-publisher check passes (the mirror publishes nothing)
#   - Guardian reaches MITIGATING, window-controller-sim reports 25 %
#   - the Gazebo window joint reaches the window state last published on
#     uProtocol (25 % = 0.10 m) within +-5 mm, observed read-only on
#     /sim/joint_states. If that bus state differs from what
#     window-controller-sim reports over HTTP, a WARN line names the known
#     publish race in window-controller-sim (reference stack)
#   - ros-up-mapper published 0 uProtocol messages and had no errors
#
# Usage:  ./ros-up-bridge/test-mirror.sh
#   COMPOSE_CMD   compose command (default podman-compose; Docker: "docker compose")
#   GUI=1         also apply gazebo-sim/compose.gui.yml (Linux, needs xhost)
#   BUILD=1       pass --build to "up"
#   BREAK=mapper  negative check: stop ros-up-mapper before the escalation;
#                 the test must then FAIL with exit code 1
#   KEEP=1        leave the stack running afterwards
set -uo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE="${COMPOSE_CMD:-podman-compose}"
FILES=(-f "${ROOT_DIR}/docker-compose.yml" -f "${ROOT_DIR}/ros-up-bridge/compose.mirror.yml")
[ "${GUI:-0}" = "1" ] && FILES+=(-f "${ROOT_DIR}/gazebo-sim/compose.gui.yml")
DC=(${COMPOSE} "${FILES[@]}" --profile gazebo)
UP_FLAGS=(-d)
[ "${BUILD:-0}" = "1" ] && UP_FLAGS+=(--build)
TOLERANCE_M="${TOLERANCE_M:-0.005}"

FAILURES=0
step() { printf '\n== %s ==\n' "$1"; }
pass() { printf 'PASS: %s\n' "$1"; }
fail() { printf 'FAIL: %s\n' "$1"; FAILURES=$((FAILURES + 1)); }

# json_get URL PYTHON_EXPR -> prints the expression evaluated on the JSON body
json_get() {
  curl -fsS --max-time 3 "$1" 2>/dev/null | python3 -c "import json,sys; d=json.load(sys.stdin); print($2)" 2>/dev/null
}

# wait_until TIMEOUT_S COMMAND... -> 0 once COMMAND succeeds
wait_until() {
  local deadline=$((SECONDS + $1)); shift
  while [ ${SECONDS} -lt ${deadline} ]; do
    "$@" >/dev/null 2>&1 && return 0
    sleep 1
  done
  return 1
}

sim_check() {
  local out rc
  out="$("${DC[@]}" exec gazebo-sim bash -lc "source /opt/ros/humble/setup.bash >/dev/null 2>&1 && python3 /opt/gazebo_sim/tools/sim_check.py $*" 2>&1)"
  rc=$?
  printf '  %s\n' "$(printf '%s' "${out}" | tr -d '\r' | tail -1)"
  return ${rc}
}

guardian_mitigating() { [ "$(json_get localhost:8080/state 'd["state"]')" = "MITIGATING" ]; }
window_at_25() { [ "$(json_get localhost:8092/state 'd["window_percentage"]')" = "25" ]; }
mapper_healthy() { curl -fsS --max-time 2 localhost:8096/health >/dev/null; }

cd "${ROOT_DIR}" || exit 1

step "static single-publisher check"
if COMPOSE_CMD="${COMPOSE}" ./ros-up-bridge/check-single-publisher.sh "${FILES[@]}" --profile gazebo; then
  pass "at most one publisher per watched topic"
else
  fail "single-publisher check"
fi

step "start: zenohd, gazebo-sim (mirror), ros-up-mapper"
"${DC[@]}" down --remove-orphans >/dev/null 2>&1
"${DC[@]}" up "${UP_FLAGS[@]}" zenohd gazebo-sim ros-up-mapper >/dev/null 2>&1
if wait_until 60 mapper_healthy; then pass "ros-up-mapper healthy"; else fail "ros-up-mapper not healthy"; fi
if sim_check joint --percent 0 --tolerance "${TOLERANCE_M}" --timeout 90; then
  pass "Gazebo up, window joint closed"
else
  fail "Gazebo not ready"
fi
bridge_ready() { "${DC[@]}" logs gazebo-sim 2>/dev/null | grep -q "mode mirror: 1 link(s)"; }
if wait_until 30 bridge_ready; then
  pass "ROS side of the bridge running in mirror mode"
else
  fail "ros_zenoh_bridge not running in gazebo-sim"
fi

REST_FLAGS=()
if [ "${BREAK:-}" = "mapper" ]; then
  echo "  (BREAK=mapper: stopping ros-up-mapper on purpose)"
  "${DC[@]}" stop ros-up-mapper >/dev/null 2>&1
  # a plain "up" would start it again
  REST_FLAGS=(--scale ros-up-mapper=0)
fi

step "start the default stack and wait for the Guardian to escalate"
"${DC[@]}" up "${UP_FLAGS[@]}" "${REST_FLAGS[@]}" >/dev/null 2>&1
if wait_until 120 guardian_mitigating && wait_until 60 window_at_25; then
  pass "Guardian MITIGATING, window-controller-sim at 25 %"
else
  fail "Guardian/window did not reach MITIGATING / 25 % (guardian=$(json_get localhost:8080/state 'd["state"]'), window=$(json_get localhost:8092/state 'd["window_percentage"]'))"
  # Diagnostics for the rare, not yet explained stall at MONITORING
  # (see README, "Known limitations").
  for svc in guardian temperature-sim child-presence-sim; do
    echo "  --- last log lines of ${svc}:"
    "${DC[@]}" logs --no-log-prefix "${svc}" 2>&1 | sed 's/\x1b\[[0-9;]*m//g' | tail -5 | sed 's/^/  /'
  done
fi

step "Gazebo mirrors the window state on uProtocol (tolerance ${TOLERANCE_M} m)"
HTTP_PCT="$(json_get localhost:8092/state 'd["window_percentage"]')"
BUS_PCT="$(json_get localhost:8096/stats 'd["routes"]["mirror_window_position"]["last_input"]["window_percentage"]')"
echo "  window-controller-sim: HTTP ${HTTP_PCT:-?} %, last published on uProtocol ${BUS_PCT:-?} %"
if [ -n "${BUS_PCT}" ] && [ -n "${HTTP_PCT}" ] && [ "${BUS_PCT}" != "${HTTP_PCT}" ]; then
  echo "WARN: known bug in window-controller-sim (reference stack): its window and alarm"
  echo "      listeners publish concurrently, so a stale state (${BUS_PCT} %) was published last."
  echo "      Gazebo correctly mirrors the bus; temperature-sim sees the same stale value."
fi
if sim_check joint --percent "${BUS_PCT:-25}" --tolerance "${TOLERANCE_M}" --timeout 30; then
  pass "Gazebo window joint at the published ${BUS_PCT:-25} %"
else
  fail "Gazebo window joint did not follow the published ${BUS_PCT:-25} %"
fi

step "mirror publishes nothing on uProtocol"
STATS="$(curl -fsS --max-time 3 localhost:8096/stats 2>/dev/null)"
if [ -n "${STATS}" ]; then
  printf '  %s\n' "${STATS}"
  UP_PUB="$(printf '%s' "${STATS}" | python3 -c 'import json,sys; print(json.load(sys.stdin)["uprotocol_published"])')"
  ERRS="$(printf '%s' "${STATS}" | python3 -c 'import json,sys; print(sum(r["errors"] for r in json.load(sys.stdin)["routes"].values()))')"
  [ "${UP_PUB}" = "0" ] && pass "uprotocol_published = 0" || fail "uprotocol_published = ${UP_PUB}"
  [ "${ERRS}" = "0" ] && pass "no route errors" || fail "route errors = ${ERRS}"
else
  fail "no /stats from ros-up-mapper"
fi

if [ "${KEEP:-0}" != "1" ]; then
  "${DC[@]}" down --remove-orphans >/dev/null 2>&1
fi

printf '\n'
if [ ${FAILURES} -eq 0 ]; then
  echo "ALL CHECKS PASSED"
else
  echo "${FAILURES} CHECK(S) FAILED"
  exit 1
fi
