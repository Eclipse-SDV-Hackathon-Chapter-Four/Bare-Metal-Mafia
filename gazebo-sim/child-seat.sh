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

# Put the "child seat" test object on the rear seat (row 2) or take it off.
#
# Uses the Gazebo Fortress UserCommands service /world/cabin/set_pose inside
# the running gazebo-sim container. "remove" puts the box back on the ground
# beside the car, outside the cabin, so nothing touches the seat cushion.
#
# Usage:  ./gazebo-sim/child-seat.sh place|remove
#   COMPOSE_CMD overrides the compose command (default: podman-compose;
#   use COMPOSE_CMD="docker compose" for Docker).
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE="${COMPOSE_CMD:-podman-compose}"
COMPOSE_FILE="${ROOT_DIR}/docker-compose.yml"
EXEC_SIM=(${COMPOSE} --profile gazebo -f "${COMPOSE_FILE}" exec gazebo-sim)

# The child_seat model origin is the bottom of its base. Seat cushion top is
# z = 0.625 in worlds/cabin.sdf, so the model goes 1 cm above the cushion
# (or the ground) and drops onto it.
case "${1:-}" in
  place)  POSE='name: "child_seat", position: {x: -0.6, y: 0.0, z: 0.635}, orientation: {w: 1}' ;;
  remove) POSE='name: "child_seat", position: {x: -0.6, y: 1.4, z: 0.01}, orientation: {w: 1}' ;;
  *) echo "usage: $0 place|remove" >&2; exit 2 ;;
esac

REPLY="$("${EXEC_SIM[@]}" ign service -s /world/cabin/set_pose \
  --reqtype ignition.msgs.Pose --reptype ignition.msgs.Boolean \
  --timeout 3000 --req "${POSE}" 2>&1 | tr -d '\r')"

if printf '%s\n' "${REPLY}" | grep -q "data: true"; then
  echo "child seat: $1 OK"
else
  echo "child seat: $1 FAILED: ${REPLY}" >&2
  exit 1
fi
