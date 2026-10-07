#!/usr/bin/env bash
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
