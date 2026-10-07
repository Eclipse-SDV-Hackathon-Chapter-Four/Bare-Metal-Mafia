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
#
# One entry point for the Gazebo cabin simulation. See gazebo-sim/README.md.

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT}" || exit 1
read -r -a COMPOSE <<< "${COMPOSE_CMD:-docker compose}"

usage() {
  cat <<'EOF'
usage: ./gazebo-sim/run.sh <command> [--gui|--no-gui] [--hvac]

  sim              Gazebo only (plus zenohd and gazebo-bridge for /state)
  mirror           full Guardian stack, Gazebo follows window-controller-sim
  replace          full Guardian stack, Gazebo replaces window-controller-sim
                   and child-presence-sim
  demo             replace + guided story with a status line every 2 s
                   (with --hvac --fault: the HVAC breaks, the window opens)
  test             end-to-end checks (replace, then mirror); exit 1 on failure

  seat place|remove   put the child seat on the rear seat / back outside
  window <0-100>      set the window opening in percent
  fault on|off        inject / clear the HVAC fault (needs --hvac stack)
  status              one status line
  logs [service...]   follow logs (default: gazebo-sim gazebo-bridge)
  down                stop everything

  --gui / --no-gui    Gazebo window (default: on when DISPLAY is set on Linux)
  --hvac              also start the ROS 2 HVAC (Eclipse Muto), ~3 min to deploy
  --fault             demo only: inject the HVAC fault once the child is seated

Images are rebuilt on every start (cached: seconds; first time: 10-30 min).
COMPOSE_CMD overrides the compose command (default: "docker compose").
Dashboard http://localhost:8094 · bridge http://localhost:8096/state
EOF
}

CMD=""
ARGS=()
GUI=auto
HVAC=0
FAULT=0
for arg in "$@"; do
  case "${arg}" in
    --gui) GUI=1 ;;
    --no-gui) GUI=0 ;;
    --hvac) HVAC=1 ;;
    --fault) FAULT=1 ;;
    -h|--help|help) usage; exit 0 ;;
    *) if [ -z "${CMD}" ]; then CMD="${arg}"; else ARGS+=("${arg}"); fi ;;
  esac
done
[ -n "${CMD}" ] || { usage; exit 2; }
if [ "${GUI}" = auto ]; then
  if [ "$(uname -s)" = Linux ] && [ -n "${DISPLAY:-}" ]; then GUI=1; else GUI=0; fi
fi

FILES=(-f docker-compose.yml)
[ "${GUI}" = 1 ] && FILES+=(-f gazebo-sim/compose.gui.yml)
PROFILES=(--profile gazebo)
[ "${HVAC}" = 1 ] && PROFILES+=(--profile ros2)
dc() { "${COMPOSE[@]}" "${FILES[@]}" "${PROFILES[@]}" "$@"; }

CORE=(zenohd guardian dashboard actuation-adapter cda-sim temperature-sim notification)
REPLACED=(window-controller-sim child-presence-sim)
GAZEBO=(gazebo-sim gazebo-bridge)
HVAC_SERVICES=(artifact-server ros2-hvac)

say() { printf '\n>> %s\n' "$*"; }
die() { printf 'ERROR: %s\n' "$*" >&2; exit 1; }

# ---------------------------------------------------------------- helpers

# json URL EXPR: fetch JSON and print a Python expression over it (d = data)
json() {
  curl -fsS --max-time 2 "$1" 2>/dev/null \
    | python3 -c 'import json, sys; d = json.load(sys.stdin); print(eval(sys.argv[1]))' "$2" 2>/dev/null
}

# wait_for SECONDS COMMAND...: retry COMMAND every second
wait_for() {
  local deadline=$((SECONDS + $1)); shift
  until "$@"; do
    [ "${SECONDS}" -ge "${deadline}" ] && return 1
    sleep 1
  done
}

gazebo_alive() { [ "$(json localhost:8096/state 'd["gazebo_alive"]')" = True ]; }

preflight() {
  if [ "${COMPOSE[0]}" = docker ] && ! docker info >/dev/null 2>&1; then
    if docker info 2>&1 | grep -q 'permission denied'; then
      die "no access to Docker. Run: sudo usermod -aG docker \$USER, then log out and in again."
    fi
    die "Docker is not running. Try: sudo systemctl enable --now docker"
  fi
  if [ "${GUI}" = 1 ] && command -v xhost >/dev/null; then
    xhost +local: >/dev/null 2>&1 || true
  fi
}

# start MODE SERVICE...: build, stop what must not run, start exactly SERVICE...
start() {
  local mode="$1"; shift
  export GAZEBO_MODE="${mode}"
  preflight
  say "Building images (first time 10-30 min, then seconds)"
  dc build "$@" || die "build failed"
  # Stop whatever runs but is not wanted now (e.g. the replaced simulators
  # after a mirror run, or temperature-sim before a test).
  local running svc
  running="$(dc ps --services --status running 2>/dev/null)"
  for svc in ${running}; do
    if ! printf '%s\n' "$@" | grep -qx "${svc}"; then
      dc rm -sf "${svc}" >/dev/null 2>&1
    fi
  done
  say "Starting: $*"
  # --remove-orphans: containers of services that no longer exist (e.g.
  # ros-up-mapper from an older checkout) would hold ports like 8092.
  dc up -d --no-deps --remove-orphans "$@" || die "start failed"
  say "Waiting for Gazebo (world, ROS 2 and both bridges)"
  wait_for 180 gazebo_alive || die "Gazebo did not come up. Look at: ./gazebo-sim/run.sh logs"
  echo "   Gazebo is up (mode ${mode}, GUI $([ "${GUI}" = 1 ] && echo on || echo off))."
}

stack() {  # services for mirror/replace, plus HVAC when requested
  local mode="$1"
  local list=("${CORE[@]}" "${GAZEBO[@]}")
  [ "${mode}" = mirror ] && list+=("${REPLACED[@]}")
  [ "${HVAC}" = 1 ] && list+=("${HVAC_SERVICES[@]}")
  printf '%s\n' "${list[@]}"
}

seat() {
  local pose
  case "${1:-}" in
    place)  pose='name: "child_seat", position: {x: -0.6, y: 0.0, z: 0.635}, orientation: {w: 1}' ;;
    remove) pose='name: "child_seat", position: {x: -0.6, y: 1.4, z: 0.01}, orientation: {w: 1}' ;;
    *) die "usage: seat place|remove" ;;
  esac
  local reply
  reply="$(dc exec -T gazebo-sim ign service -s /world/cabin/set_pose \
    --reqtype ignition.msgs.Pose --reptype ignition.msgs.Boolean \
    --timeout 3000 --req "${pose}" 2>&1 | tr -d '\r')"
  if printf '%s\n' "${reply}" | grep -q 'data: true'; then
    echo "   child seat: $1"
  else
    echo "   child seat: $1 FAILED: ${reply}" >&2
    return 1
  fi
}

window() {
  local pct="${1:-}"
  [[ "${pct}" =~ ^[0-9]+$ ]] && [ "${pct}" -le 100 ] || die "usage: window <0-100>"
  if curl -fsS --max-time 2 localhost:8094/health >/dev/null 2>&1; then
    # Full stack: a UDS window command, like the dashboard's buttons. Goes to
    # window-controller-sim (mirror) or gazebo-bridge (replace).
    curl -fsS --max-time 3 -X POST localhost:8094/api/window \
      -H 'Content-Type: application/json' -d "{\"percentage\": ${pct}}" >/dev/null \
      || die "dashboard did not accept the window command"
  else
    # Gazebo only: straight onto the bridge's Zenoh key.
    dc exec -T gazebo-sim python3 -c "
import json, os, time, zenoh
c = zenoh.Config()
c.insert_json5('mode', '\"client\"')
c.insert_json5('connect/endpoints', json.dumps([os.environ['ZENOH_CONNECT']]))
s = zenoh.open(c); time.sleep(0.5)
s.put('gazebo/window/cmd', json.dumps({'percent': ${pct}})); time.sleep(0.2); s.close()" \
      || die "could not send the window command"
  fi
  echo "   window: ${pct} %"
}

fault() {
  local on
  case "${1:-}" in on) on=true ;; off) on=false ;; *) die "usage: fault on|off" ;; esac
  curl -fsS --max-time 3 -X POST localhost:18081/api/fault \
    -H 'Content-Type: application/json' -d "{\"fault_active\": ${on}}" >/dev/null \
    || die "HVAC console not reachable; start with --hvac"
  echo "   HVAC fault: $1"
}

status_line() {
  local d b
  d="$(curl -fsS --max-time 2 localhost:8094/api/state 2>/dev/null || echo '{}')"
  b="$(curl -fsS --max-time 2 localhost:8096/state 2>/dev/null || echo '{}')"
  python3 - "${d}" "${b}" <<'PY'
import json, sys
d, b = json.loads(sys.argv[1]), json.loads(sys.argv[2])
g = d.get("guardian") or {}
t = (d.get("cabin_temperature") or {}).get("temperature_celsius")
w = d.get("window_state") or {}
h = d.get("hvac_state")
hvac = "-"
if h:
    hvac = ("AC on" if h.get("air_conditioning_active") else "AC off") + \
        " %sC fan %s%%" % (h.get("target_temperature_celsius"), h.get("fan_speed_percent")) + \
        (" FAULT" if h.get("fault_active") else "")
print("%-11s child %-5s  cabin %5s C  window %3s %% (glass %3s %%)  alarm %-5s  HVAC %s" % (
    g.get("state", "?"), g.get("child_present", "?"),
    "?" if t is None else "%.1f" % t,
    w.get("window_percentage", "?"), b.get("window_percentage", "?"),
    w.get("alarm_enabled", "?"), hvac))
PY
}

watch_for() {
  local end=$((SECONDS + $1))
  while [ "${SECONDS}" -lt "${end}" ]; do
    printf '   %3ss  %s\n' "$((SECONDS - T0))" "$(status_line)"
    sleep 2
  done
}

# ---------------------------------------------------------------- tests

FAILS=0
pass() { printf '   PASS  %s\n' "$*"; }
fail() { printf '   FAIL  %s\n' "$*"; FAILS=$((FAILS + 1)); }
check() {  # check SECONDS "description" COMMAND...
  local secs="$1" what="$2"; shift 2
  if wait_for "${secs}" "$@"; then pass "${what}"; else fail "${what}"; fi
}
eq() { [ "$(json "$1" "$2")" = "$3" ]; }
answers() { [ -n "$(json "$1" "$2")" ]; }
near() {  # near URL EXPR TARGET: |value - TARGET| <= 2
  local v; v="$(json "$1" "$2")"
  [[ "${v}" =~ ^[0-9]+$ ]] && [ $((v > $3 ? v - $3 : $3 - v)) -le 2 ]
}
not_running() { ! dc ps --status running --services 2>/dev/null | grep -qx "$1"; }

test_replace() {
  say "TEST replace: Gazebo is the window actuator and the child sensor"
  local svcs; mapfile -t svcs < <(stack replace | grep -vx temperature-sim)
  start replace "${svcs[@]}"
  check 1 "window-controller-sim is not running" not_running window-controller-sim
  check 1 "child-presence-sim is not running" not_running child-presence-sim
  check 20 "Guardian is up" answers localhost:8080/state 'd["state"]'
  seat remove >/dev/null
  check 5 "empty seat -> Guardian child_present false" eq localhost:8080/state 'd["child_present"]' False
  seat place >/dev/null
  check 5 "child seat placed -> Guardian child_present true" eq localhost:8080/state 'd["child_present"]' True
  window 25 >/dev/null
  check 10 "window 25 % -> Gazebo glass at 25 %" near localhost:8096/state 'd["window_percentage"]' 25
  check 5 "dashboard (uProtocol) shows window 25 %" eq localhost:8094/api/state 'd["window_state"]["window_percentage"]' 25
  window 0 >/dev/null
  check 10 "window 0 % -> Gazebo glass closed" near localhost:8096/state 'd["window_percentage"]' 0
  seat remove >/dev/null
  check 5 "child seat removed -> Guardian child_present false" eq localhost:8080/state 'd["child_present"]' False
}

test_mirror() {
  say "TEST mirror: Gazebo follows window-controller-sim"
  local svcs; mapfile -t svcs < <(stack mirror | grep -vx temperature-sim)
  start mirror "${svcs[@]}"
  check 20 "window-controller-sim is up" answers localhost:8092/state 'd["window_percentage"]'
  window 25 >/dev/null
  check 5 "window-controller-sim reports 25 %" eq localhost:8092/state 'd["window_percentage"]' 25
  check 10 "Gazebo glass follows to 25 %" near localhost:8096/state 'd["window_percentage"]' 25
  check 1 "bridge mode is mirror" eq localhost:8096/state 'd["mode"]' mirror
  window 0 >/dev/null
  check 10 "Gazebo glass follows back to 0 %" near localhost:8096/state 'd["window_percentage"]' 0
}

# ---------------------------------------------------------------- commands

case "${CMD}" in
  sim)
    start mirror zenohd "${GAZEBO[@]}"
    echo "   Try: ./gazebo-sim/run.sh seat place   ·   ./gazebo-sim/run.sh window 100"
    ;;
  mirror|replace)
    mapfile -t svcs < <(stack "${CMD}")
    start "${CMD}" "${svcs[@]}"
    echo "   Dashboard: http://localhost:8094   Status: ./gazebo-sim/run.sh status"
    ;;
  demo)
    [ "${FAULT}" = 1 ] && [ "${HVAC}" = 0 ] && die "--fault needs --hvac"
    mapfile -t svcs < <(stack replace | grep -vx temperature-sim)
    start replace "${svcs[@]}"
    if [ "${HVAC}" = 1 ]; then
      say "Waiting for the ROS 2 HVAC (Eclipse Muto deploys it, up to 3 min)"
      wait_for 180 curl -fs --max-time 2 localhost:18081/api/state -o /dev/null \
        || echo "   HVAC console not reachable, continuing without it"
      fault off >/dev/null 2>&1
    fi
    seat remove >/dev/null
    window 0 >/dev/null
    # fresh Guardian, so no state from an earlier run shows up
    dc up -d --no-deps --force-recreate guardian >/dev/null 2>&1
    wait_for 20 answers localhost:8080/state 'd["state"]'
    T0=${SECONDS}
    echo "   Dashboard http://localhost:8094$([ "${HVAC}" = 1 ] && echo '   HVAC console http://localhost:18081')"
    watch_for 4
    say "A child is put on the rear seat (Gazebo seat contact sensor)"
    seat place
    watch_for 6
    if [ "${FAULT}" = 1 ]; then
      say "The HVAC breaks down"
      fault on
    fi
    say "The parked car heats up (temperature-sim)"
    dc up -d --no-deps temperature-sim >/dev/null 2>&1
    watch_for "${DURATION:-60}"
    [ "${FAULT}" = 1 ] && fault off
    say "Manual override from the dashboard: window 100 %, then closed again"
    window 100
    watch_for 6
    window 0
    watch_for 4
    say "The child is taken out of the car"
    seat remove
    watch_for 6
    say "Done. The stack keeps running. Stop it with: ./gazebo-sim/run.sh down"
    ;;
  test)
    what="${ARGS[0]:-all}"
    case "${what}" in
      replace) test_replace ;;
      mirror) test_mirror ;;
      all) test_replace; test_mirror ;;
      *) die "usage: test [replace|mirror|all]" ;;
    esac
    echo
    if [ "${FAILS}" -eq 0 ]; then echo "ALL PASSED"; else echo "${FAILS} CHECK(S) FAILED"; exit 1; fi
    ;;
  seat) seat "${ARGS[0]:-}" ;;
  window) window "${ARGS[0]:-}" ;;
  fault) fault "${ARGS[0]:-}" ;;
  status) status_line ;;
  logs)
    if [ "${#ARGS[@]}" -eq 0 ]; then ARGS=("${GAZEBO[@]}"); fi
    dc logs -f --tail 50 "${ARGS[@]}"
    ;;
  down)
    "${COMPOSE[@]}" -f docker-compose.yml --profile gazebo --profile ros2 down --remove-orphans
    ;;
  *) usage; exit 2 ;;
esac
