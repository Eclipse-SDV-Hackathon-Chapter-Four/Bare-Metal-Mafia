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

# Guided demo of the Gazebo cabin: watch it in the GUI while this runs.
#
#   child outside -> child placed on the rear seat (contact sensor fires)
#   -> window 25 % -> 100 % -> closed -> child taken out (contact stops)
#
# Start the simulation with the GUI first (Linux), see gazebo-sim/README.md:
#   xhost +local:
#   docker compose -f docker-compose.yml -f gazebo-sim/compose.gui.yml --profile gazebo up -d gazebo-sim
#
# Usage:  ./gazebo-sim/demo.sh
#   COMPOSE_CMD overrides the compose command (default: podman-compose;
#   use COMPOSE_CMD="docker compose" for Docker).
#   DEMO_PAUSE_S sets the pause between steps (default 3).
set -uo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE="${COMPOSE_CMD:-podman-compose}"
COMPOSE_FILE="${ROOT_DIR}/docker-compose.yml"
PAUSE="${DEMO_PAUSE_S:-3}"

EXEC_SIM=(${COMPOSE} --profile gazebo -f "${COMPOSE_FILE}" exec gazebo-sim)
CHILD_SEAT="${ROOT_DIR}/gazebo-sim/child-seat.sh"

say() { printf '\n>> %s\n' "$1"; }

sim_check() {
  "${EXEC_SIM[@]}" bash -lc "source /opt/ros/humble/setup.bash >/dev/null 2>&1 && python3 /opt/gazebo_sim/tools/sim_check.py $*" 2>&1 \
    | tr -d '\r' | tail -1 | sed 's/^/   /'
}

if ! "${EXEC_SIM[@]}" true >/dev/null 2>&1; then
  echo "gazebo-sim is not running; start it first (see the header of this script)" >&2
  exit 1
fi

say "Start: window closed, child seat outside the car"
"${CHILD_SEAT}" remove >/dev/null
sim_check window --percent 0
sleep "${PAUSE}"

say "A child in a child seat is put on the rear seat (row 2)"
"${CHILD_SEAT}" place | sed 's/^/   /'
sim_check contact --expect present
sleep "${PAUSE}"

say "Cabin is getting hot: open the rear-left window to 25 % (Guardian stage 2)"
sim_check window --percent 25
sleep "${PAUSE}"

say "Open the window fully (100 %)"
sim_check window --percent 100
sleep "${PAUSE}"

say "Close the window again (0 %)"
sim_check window --percent 0
sleep "${PAUSE}"

say "The child is taken out of the car"
"${CHILD_SEAT}" remove | sed 's/^/   /'
sleep 1
sim_check contact --expect absent --timeout 3

say "Demo done"
