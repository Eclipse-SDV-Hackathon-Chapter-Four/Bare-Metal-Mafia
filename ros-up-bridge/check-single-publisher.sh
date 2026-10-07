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

# Static single-publisher check: there must never be two publishers for the
# rear-left window state (TOPIC_WINDOW_STATE, URI vss_window_state) or for
# child presence (TOPIC_CHILD_PRESENCE, URI vss_child_presence).
#
# Renders the effective compose configuration for the given compose files /
# profiles and counts, per topic, the services that would publish it:
#   window-controller-sim  -> vss_window_state
#   child-presence-sim     -> vss_child_presence
#   ros-up-mapper          -> every uProtocol output ("to: {uprotocol: ...}")
#                             in the mapping file the service is started with
#
# Usage (arguments are passed to "<compose> config"):
#   ./ros-up-bridge/check-single-publisher.sh                        # default stack
#   ./ros-up-bridge/check-single-publisher.sh -f docker-compose.yml \
#       -f ros-up-bridge/compose.replace.yml --profile gazebo
#   COMPOSE_CMD overrides the compose command (default: podman-compose;
#   use COMPOSE_CMD="docker compose" for Docker).
# Exit code 0 = at most one publisher per topic, 1 = conflict or error.
set -uo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE="${COMPOSE_CMD:-podman-compose}"

cd "${ROOT_DIR}" || exit 1
if ! CONFIG_JSON="$(${COMPOSE} "$@" config --format json 2>/dev/null)"; then
  echo "FAIL: '${COMPOSE} $* config' failed"
  exit 1
fi

printf '%s' "${CONFIG_JSON}" | python3 -c '
import json, os, sys
import yaml

WATCHED = ("vss_window_state", "vss_child_presence")
SIMULATORS = {"window-controller-sim": "vss_window_state",
              "child-presence-sim": "vss_child_presence"}
CONTAINER_CONFIG_DIR = "/opt/ros_up_bridge/config/"

def mapping_path(service):
    cmd = service.get("command") or []
    if isinstance(cmd, str):
        cmd = cmd.split()
    path = None
    if "--config" in cmd and cmd.index("--config") + 1 < len(cmd):
        path = cmd[cmd.index("--config") + 1]
    path = path or (service.get("environment") or {}).get("ROS_UP_MAPPER_CONFIG")
    path = path or CONTAINER_CONFIG_DIR + "mirror.yaml"  # image default
    if path.startswith(CONTAINER_CONFIG_DIR):
        path = os.path.join("ros-up-bridge/config", path[len(CONTAINER_CONFIG_DIR):])
    return path

def uprotocol_outputs(path):
    with open(path) as f:
        cfg = yaml.safe_load(f)
    out = []
    for route in cfg.get("routes", []):
        to = route.get("to") or {}
        if isinstance(to.get("uprotocol"), str):
            out.append(to["uprotocol"])
    return out

services = json.load(sys.stdin).get("services", {})
publishers = {topic: [] for topic in WATCHED}
for name, svc in services.items():
    if name in SIMULATORS:
        publishers[SIMULATORS[name]].append(name)
    if "ros-up-mapper" in (svc.get("image") or ""):
        path = mapping_path(svc)
        for topic in uprotocol_outputs(path):
            if topic in publishers:
                publishers[topic].append(f"{name} ({os.path.basename(path)})")

failed = False
for topic, pubs in publishers.items():
    status = "PASS" if len(pubs) <= 1 else "FAIL"
    failed |= status == "FAIL"
    listed = ", ".join(pubs) or "-"
    print(f"{status}: {topic}: {len(pubs)} publisher(s): {listed}")
sys.exit(1 if failed else 0)
'
