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

# End-to-end test for ros-up-bridge mode "replace": Gazebo replaces
# window-controller-sim and child-presence-sim.
#
# (Re)starts the stack itself, with ros-up-mapper in strict single-publisher
# mode, and holds back temperature-sim so the Guardian's states come in a
# reproducible order:
#   1. static single-publisher check: PASS for replace, FAIL when the
#      replaced simulators are re-enabled
#   2. zenohd, gazebo-sim (bridge mode replace), ros-up-mapper; then the rest
#      of the stack without temperature-sim; the replaced simulators must
#      not be running
#   3. child seat placed in Gazebo  -> Guardian MONITORING
#   4. temperature-sim started     -> WARNING -> CRITICAL -> MITIGATING
#   5. stage-2 mitigation (window 25 %) moves the Gazebo glass to 25 %; the
#      window state on uProtocol now comes from Gazebo (HTTP /state of the
#      mapper and the dashboard, a uProtocol subscriber, both show 25 %)
#   6. window state events while the glass moved: >= 1 percentage point
#      apart, >= 200 ms apart (<= 5 Hz), final value 25 %
#   7. child seat removed          -> Guardian CLEAR
#   8. exactly one publisher per topic at runtime: the mapper (strict mode)
#      is still running, saw no foreign message, /health is ok
#
# Usage:  ./ros-up-bridge/test-replace.sh
#   COMPOSE_CMD    compose command (default podman-compose; Docker: "docker compose")
#   GUI=1          also apply gazebo-sim/compose.gui.yml (Linux, needs xhost)
#   BUILD=1        pass --build to "up"
#   BREAK=foreign  negative check: also start window-controller-sim, a second
#                  publisher of the window state; the test must then FAIL
#                  with exit code 1
#   KEEP=1         leave the stack running afterwards
set -uo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE="${COMPOSE_CMD:-podman-compose}"
FILES=(-f "${ROOT_DIR}/docker-compose.yml" -f "${ROOT_DIR}/ros-up-bridge/compose.replace.yml")
[ "${GUI:-0}" = "1" ] && FILES+=(-f "${ROOT_DIR}/gazebo-sim/compose.gui.yml")
DC=(${COMPOSE} "${FILES[@]}" --profile gazebo)
UP_FLAGS=(-d)
[ "${BUILD:-0}" = "1" ] && UP_FLAGS+=(--build)
TOLERANCE_M="${TOLERANCE_M:-0.005}"
export ROS_UP_STRICT_SINGLE_PUBLISHER=1
export ROS_UP_MAPPER_LOG="ros_up_mapper=debug,info"

FAILURES=0
step() { printf '\n== %s ==\n' "$1"; }
pass() { printf 'PASS: %s\n' "$1"; }
fail() { printf 'FAIL: %s\n' "$1"; FAILURES=$((FAILURES + 1)); }

json_get() {
  curl -fsS --max-time 3 "$1" 2>/dev/null | python3 -c "import json,sys; d=json.load(sys.stdin); print($2)" 2>/dev/null
}

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

child_seat() { COMPOSE_CMD="${COMPOSE}" "${ROOT_DIR}/gazebo-sim/child-seat.sh" "$1" >/dev/null 2>&1; }
guardian_state() { json_get localhost:8080/state 'd["state"]'; }
guardian_is() { [ "$(guardian_state)" = "$1" ]; }
mapper_healthy() { curl -fsS --max-time 2 localhost:8092/health >/dev/null; }
bridge_ready() { "${DC[@]}" logs gazebo-sim 2>/dev/null | grep -q "mode replace: 3 link(s)"; }
window_state_is() { [ "$(json_get localhost:8092/state 'd["window_percentage"]')" = "$1" ]; }
dashboard_window_is() { [ "$(json_get localhost:8094/api/state '(d.get("window_state") or {}).get("window_percentage")')" = "$1" ]; }
running() { "${DC[@]}" ps --status running --services 2>/dev/null | grep -qx "$1"; }

cd "${ROOT_DIR}" || exit 1

step "static single-publisher check"
if COMPOSE_CMD="${COMPOSE}" ./ros-up-bridge/check-single-publisher.sh "${FILES[@]}" --profile gazebo; then
  pass "one publisher per watched topic in replace mode"
else
  fail "single-publisher check (replace)"
fi
if COMPOSE_CMD="${COMPOSE}" ./ros-up-bridge/check-single-publisher.sh "${FILES[@]}" --profile gazebo --profile replaced-by-gazebo >/dev/null 2>&1; then
  fail "static check did not flag re-enabled simulators"
else
  pass "static check flags re-enabled simulators (expected FAIL detected)"
fi

step "start: zenohd, gazebo-sim (replace), ros-up-mapper (strict)"
docker rm -f ros-up-test-second-publisher >/dev/null 2>&1
"${DC[@]}" --profile replaced-by-gazebo down --remove-orphans >/dev/null 2>&1
"${DC[@]}" up "${UP_FLAGS[@]}" zenohd gazebo-sim ros-up-mapper >/dev/null 2>&1
if wait_until 60 mapper_healthy; then pass "ros-up-mapper healthy (strict single-publisher mode)"; else fail "ros-up-mapper not healthy"; fi
if sim_check joint --percent 0 --tolerance "${TOLERANCE_M}" --timeout 90; then
  pass "Gazebo up, window closed"
else
  fail "Gazebo not ready"
fi
if wait_until 30 bridge_ready; then pass "ROS side of the bridge running in replace mode"; else fail "ros_zenoh_bridge not running in replace mode"; fi
child_seat remove

step "start the stack without temperature-sim"
"${DC[@]}" up "${UP_FLAGS[@]}" --scale temperature-sim=0 >/dev/null 2>&1
for svc in window-controller-sim child-presence-sim; do
  if running "${svc}"; then fail "${svc} is running"; else pass "${svc} not running (replaced by Gazebo)"; fi
done
if wait_until 30 guardian_is CLEAR; then pass "Guardian CLEAR (empty seat)"; else fail "Guardian not CLEAR: $(guardian_state)"; fi

if [ "${BREAK:-}" = "foreign" ]; then
  echo "  (BREAK=foreign: starting window-controller-sim as a second publisher on purpose)"
  # "run" publishes no ports (8092 belongs to the mapper in replace mode)
  "${DC[@]}" --profile replaced-by-gazebo run -d --no-deps --name ros-up-test-second-publisher \
    window-controller-sim >/dev/null 2>&1
  sleep 3
  docker ps --filter name=ros-up-test-second-publisher --format '  second publisher container: {{.Status}}'
fi

step "child placed on the rear seat"
child_seat place
if wait_until 15 guardian_is MONITORING; then
  pass "Guardian MONITORING ($(json_get localhost:8080/state 'd["child_present"]'))"
else
  fail "Guardian not MONITORING after child placed: $(guardian_state)"
fi

step "cabin heats up (temperature-sim started)"
STATS_BEFORE="$(json_get localhost:8092/stats 'd["routes"]["window_state"]["sent"]')"
"${DC[@]}" up -d --no-deps temperature-sim >/dev/null 2>&1
if wait_until 60 guardian_is MITIGATING; then pass "Guardian MITIGATING"; else fail "Guardian not MITIGATING: $(guardian_state)"; fi
step "window mitigation moves the Gazebo glass; state comes from Gazebo"
if wait_until 60 window_state_is 25; then pass "window state 25 % (mapper /state, from the joint)"; else fail "window state not 25 %: $(json_get localhost:8092/state 'd')"; fi
if sim_check joint --percent 25 --tolerance "${TOLERANCE_M}" --timeout 30; then
  pass "Gazebo window joint at 25 %"
else
  fail "Gazebo window joint not at 25 %"
fi
if [ "$(json_get localhost:8092/state 'd["alarm_enabled"]')" = "True" ]; then pass "alarm_enabled true"; else fail "alarm_enabled not true"; fi
if wait_until 10 dashboard_window_is 25; then
  pass "dashboard (uProtocol subscriber) sees window 25 %"
else
  fail "dashboard window state: $(json_get localhost:8094/api/state 'd.get("window_state")')"
fi
# Checked after the window stage: without a working HVAC the Guardian's
# /state flips between MITIGATING and CRITICAL after the HVAC stage, and the
# log line "-> Mitigating" appears for good only with the window stage.
guardian_seq() { "${DC[@]}" logs guardian 2>/dev/null | sed 's/\x1b\[[0-9;]*m//g' | grep -oE -- '-> [A-Za-z]+$' | uniq | tr '\n' ' '; }
seq_complete() { guardian_seq | grep -q -- "-> Monitoring -> Warning -> Critical -> Mitigating"; }
wait_until 10 seq_complete
SEQ="$(guardian_seq)"
echo "  Guardian states: ${SEQ}"
if printf '%s' "${SEQ}" | grep -q -- "-> Monitoring -> Warning -> Critical -> Mitigating"; then
  pass "MONITORING -> WARNING -> CRITICAL -> MITIGATING"
else
  fail "unexpected Guardian sequence"
fi

step "window state events while the glass moved"
sleep 2
"${DC[@]}" logs --no-log-prefix ros-up-mapper 2>/dev/null | sed 's/\x1b\[[0-9;]*m//g' \
  | grep -oE 'route window_state: published \{.*\}' | sed 's/^route window_state: published //' > /tmp/ros_up_window_events.$$
printf '  %s\n' "$(python3 - /tmp/ros_up_window_events.$$ <<'EOF'
import json, sys
ev = [json.loads(l) for l in open(sys.argv[1]) if l.strip()]
moving = [e for e in ev if e["alarm_enabled"]]          # stage 2 onwards
pct = [e["window_percentage"] for e in moving]
gaps = [b["timestamp_ms"] - a["timestamp_ms"] for a, b in zip(moving, moving[1:])]
print(f"{len(ev)} events total, {len(moving)} since alarm on: {pct}, min gap {min(gaps) if gaps else '-'} ms")
EOF
)"
if python3 - /tmp/ros_up_window_events.$$ <<'EOF'
import json, sys
ev = [json.loads(l) for l in open(sys.argv[1]) if l.strip()]
m = [e for e in ev if e["alarm_enabled"]]
ok = len(m) >= 2 and m[-1]["window_percentage"] == 25
ok &= all(b["timestamp_ms"] - a["timestamp_ms"] >= 195 for a, b in zip(m, m[1:]))
moves = [(a, b) for a, b in zip(m, m[1:]) if a["window_percentage"] != b["window_percentage"]]
ok &= all(abs(a["window_percentage"] - b["window_percentage"]) >= 1 for a, b in moves)
ok &= any(0 < e["window_percentage"] < 25 for e in m)   # intermediate values were published
sys.exit(0 if ok else 1)
EOF
then
  pass "intermediate values published, >= 1 point and >= 200 ms apart, final 25 %"
else
  fail "window state throttling / final value"
fi
rm -f /tmp/ros_up_window_events.$$

step "child removed"
child_seat remove
if wait_until 15 guardian_is CLEAR; then pass "Guardian CLEAR"; else fail "Guardian not CLEAR after child removed: $(guardian_state)"; fi
echo "  window stays at $(json_get localhost:8092/state 'd["window_percentage"]') % (the Guardian sends no 'close' on CLEAR; existing behaviour)"

step "exactly one publisher per topic at runtime"
for svc in window-controller-sim child-presence-sim; do
  if running "${svc}"; then fail "${svc} is running"; else pass "${svc} not running"; fi
done
STATS="$(curl -fsS --max-time 3 localhost:8092/stats 2>/dev/null)"
if [ -n "${STATS}" ] && running ros-up-mapper; then
  FOREIGN="$(printf '%s' "${STATS}" | python3 -c 'import json,sys; print(json.load(sys.stdin)["foreign_publisher_events"])')"
  UP_PUB="$(printf '%s' "${STATS}" | python3 -c 'import json,sys; print(json.load(sys.stdin)["uprotocol_published"])')"
  [ "${FOREIGN}" = "0" ] && pass "no foreign publisher seen (strict mapper still running)" || fail "foreign publisher events: ${FOREIGN}"
  [ "${UP_PUB}" -gt 0 ] 2>/dev/null && pass "mapper published ${UP_PUB} uProtocol event(s)" || fail "mapper published nothing"
  mapper_healthy && pass "/health ok" || fail "/health unhealthy"
else
  fail "ros-up-mapper not running or no /stats (strict mode exits on a second publisher)"
  "${DC[@]}" logs --no-log-prefix ros-up-mapper 2>/dev/null | grep -E "SECOND PUBLISHER|STRICT" | head -2 | sed 's/^/  /'
fi

if [ "${KEEP:-0}" != "1" ]; then
  docker rm -f ros-up-test-second-publisher >/dev/null 2>&1
  "${DC[@]}" --profile replaced-by-gazebo down --remove-orphans >/dev/null 2>&1
fi

printf '\n'
if [ ${FAILURES} -eq 0 ]; then
  echo "ALL CHECKS PASSED"
else
  echo "${FAILURES} CHECK(S) FAILED"
  exit 1
fi
