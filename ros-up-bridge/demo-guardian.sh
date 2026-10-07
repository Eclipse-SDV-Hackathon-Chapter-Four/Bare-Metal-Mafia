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
# Assisted-by: Anthropic Claude Fable 5.1 (claude-fable-5-1)

# Guided demo of the whole Guardian Loop with Gazebo (bridge mode replace).
#
#   1. starts the stack: Gazebo replaces the window controller and the child
#      presence simulator; temperature-sim is held back
#   2. puts the child seat on the rear seat in Gazebo
#   3. starts temperature-sim: the cabin heats up 26 -> 36 -> 43 C
#   4. prints one live line every 2 s: Guardian state, child, cabin
#      temperature, window (from the Gazebo joint) and HVAC
#   5. takes the child out and shows the Guardian going back to CLEAR
#
# Without HVAC the Guardian's HVAC request is never confirmed, so after 12 s
# it opens the window to 25 % (the Gazebo glass moves) and raises the alarm.
# With HVAC=1 the ROS 2 HVAC workload (Eclipse Muto) answers: AC on, 18 C,
# fan 100 %, and the cabin cools without opening the window. Click "Inject
# HVAC Fault" on http://localhost:18081 to make the Guardian open the window.
#
# The stack keeps running afterwards: dashboard http://localhost:8094.
#
# Usage:  ./ros-up-bridge/demo-guardian.sh
#   COMPOSE_CMD   compose command (default podman-compose; Docker: "docker compose")
#   GUI=1         show the Gazebo GUI (Linux, run "xhost +local:" first)
#   HVAC=1        add the ROS 2 HVAC path (--profile ros2; ~30 s extra start-up)
#   BUILD=1       pass --build to "up"
#   DURATION      seconds to watch the heat-up (default 60)
set -uo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE="${COMPOSE_CMD:-podman-compose}"
FILES=(-f "${ROOT_DIR}/docker-compose.yml" -f "${ROOT_DIR}/ros-up-bridge/compose.replace.yml")
[ "${GUI:-0}" = "1" ] && FILES+=(-f "${ROOT_DIR}/gazebo-sim/compose.gui.yml")
PROFILES=(--profile gazebo)
[ "${HVAC:-0}" = "1" ] && PROFILES+=(--profile ros2)
DC=(${COMPOSE} "${FILES[@]}" "${PROFILES[@]}")
UP_FLAGS=(-d)
[ "${BUILD:-0}" = "1" ] && UP_FLAGS+=(--build)
DURATION="${DURATION:-60}"

say() { printf '\n>> %s\n' "$1"; }
child_seat() { COMPOSE_CMD="${COMPOSE}" "${ROOT_DIR}/gazebo-sim/child-seat.sh" "$1" >/dev/null 2>&1; }
wait_url() {
  local deadline=$((SECONDS + $2))
  until curl -fsS --max-time 2 "$1" >/dev/null 2>&1; do
    [ ${SECONDS} -ge ${deadline} ] && return 1
    sleep 1
  done
}

# One line from the dashboard's aggregate of the uProtocol topics.
status_line() {
  curl -fsS --max-time 2 localhost:8094/api/state 2>/dev/null | python3 -c '
import json, sys
d = json.load(sys.stdin)
g = d.get("guardian") or {}
c = d.get("child_presence") or {}
t = d.get("cabin_temperature") or {}
w = d.get("window_state") or {}
h = d.get("hvac_state")
state = g.get("state", "?")
child = str(c.get("present", "?"))
temp = t.get("temperature_celsius")
temp = "?" if temp is None else "%.1f" % temp
window = str(w.get("window_percentage", "?"))
alarm = str(w.get("alarm_enabled", "?"))
if h is None:
    hvac = "-"
else:
    hvac = "AC on" if h.get("air_conditioning_active") else "AC off"
    hvac += " %sC fan %s%%" % (h.get("target_temperature_celsius"), h.get("fan_speed_percent"))
    if h.get("fault_active"):
        hvac += " FAULT"
print("%-11s child %-5s  cabin %5s C  window %3s %%  alarm %-5s  HVAC %s" % (state, child, temp, window, alarm, hvac))
' 2>/dev/null || echo "(dashboard not reachable yet)"
}

watch_for() {
  local end=$((SECONDS + $1))
  while [ ${SECONDS} -lt ${end} ]; do
    printf '   %3ss  %s\n' "$((SECONDS - T0))" "$(status_line)"
    sleep 2
  done
}

cd "${ROOT_DIR}" || exit 1

say "Starting the stack (Gazebo replaces window controller + child presence sim)"
"${DC[@]}" --profile replaced-by-gazebo down --remove-orphans >/dev/null 2>&1
"${DC[@]}" up "${UP_FLAGS[@]}" zenohd gazebo-sim ros-up-mapper >/dev/null 2>&1
wait_url localhost:8092/health 90 || { echo "ros-up-mapper did not come up"; exit 1; }
"${DC[@]}" up "${UP_FLAGS[@]}" --scale temperature-sim=0 >/dev/null 2>&1
wait_url localhost:8094/api/state 60 || { echo "dashboard did not come up"; exit 1; }
sleep 3
if [ "${HVAC:-0}" = "1" ]; then
  say "Waiting for the ROS 2 HVAC workload (Eclipse Muto deploys it, ~30 s)"
  wait_url localhost:18081/api/state 180 || echo "   HVAC console not reachable, continuing without it"
fi
child_seat remove
T0=${SECONDS}
echo "   Dashboard: http://localhost:8094   (HVAC console: http://localhost:18081 with HVAC=1)"
watch_for 4

say "A child is put on the rear seat (Gazebo seat contact sensor)"
child_seat place
watch_for 6

say "The parked car heats up (temperature-sim: 26 -> 36 -> 43 C, then sun)"
"${DC[@]}" up -d --no-deps temperature-sim >/dev/null 2>&1
watch_for "${DURATION}"

say "The child is taken out of the car"
child_seat remove
watch_for 8

say "Done. The stack keeps running; stop it with:"
echo "   ${COMPOSE} ${FILES[*]#${ROOT_DIR}/} ${PROFILES[*]} --profile replaced-by-gazebo down" | sed "s|${ROOT_DIR}/||g"
