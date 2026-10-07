<!--
  Copyright (c) 2026 Contributors to the Bare-Metal-Mafia project
  See the NOTICE file(s) distributed with this work for additional
  information regarding copyright ownership.

  This program and the accompanying materials are made available under the
  terms of the Apache License, Version 2.0 which is available at
  https://www.apache.org/licenses/LICENSE-2.0

  AI Disclosure: This file was largely AI-generated. The AI-generated
  portions are made available under CC0-1.0 and not subject to the
  project's licence. The human contributor has reviewed and verified
  that the code is correct.

  SPDX-License-Identifier: Apache-2.0 AND CC0-1.0
  Assisted-by: Anthropic Claude Opus 5.5 (claude-opus-5-5)
-->

# gazebo-sim — Gazebo cabin simulation

A physics simulation of the car for the Guardian Loop: a rear-left window
that really moves and a rear seat with a contact sensor. Gazebo can either
**mirror** the simulated window controller or **replace** the window
controller and the child presence simulator, without any change to the
Guardian. Our own work, optional, developer laptops only.

## Quick start

Needs Docker with Compose v2, and Linux for the Gazebo window.

```bash
./gazebo-sim/run.sh demo              # Guardian Loop with Gazebo, guided, ~2 min
./gazebo-sim/run.sh test              # end-to-end checks, prints PASS/FAIL
./gazebo-sim/run.sh down              # stop everything
```

The first start builds the images (10–30 min, ~3 GB for Gazebo). Every later
start rebuilds from cache in seconds, so you never run an outdated image.

| Command | What it does |
|---|---|
| `sim` | Gazebo only (plus zenohd and gazebo-bridge for `/state`) |
| `mirror` | full stack; Gazebo follows `window-controller-sim` |
| `replace` | full stack; Gazebo replaces `window-controller-sim` and `child-presence-sim` |
| `demo` | `replace` plus a scripted story with a status line every 2 s |
| `test [replace\|mirror]` | end-to-end checks, exit code 1 on failure |
| `seat place\|remove` | child seat onto the rear seat / back outside the car |
| `window <0-100>` | window opening in percent (through the dashboard API when the stack runs) |
| `fault on\|off` | inject / clear the HVAC fault (stack started with `--hvac`) |
| `status` | one status line: Guardian, child, cabin, window, glass, HVAC |
| `logs [service…]` | follow logs, default `gazebo-sim gazebo-bridge` |
| `down` | stop everything |

Flags: `--gui` / `--no-gui` (default: GUI when `DISPLAY` is set on Linux),
`--hvac` (adds the ROS 2 HVAC via Eclipse Muto, ~3 min to deploy),
`--fault` (demo only, with `--hvac`: the HVAC breaks once the child is
seated, so the Guardian opens the window). `COMPOSE_CMD` picks another
compose command.

Watch: dashboard <http://localhost:8094>, bridge state
<http://localhost:8096/state>, HVAC console <http://localhost:18081>
(`--hvac`).

## How it fits together

```text
 gazebo-sim container (ROS_DOMAIN_ID 42)                         services
┌──────────────────────────────────────────────┐  Zenoh    ┌───────────────┐ uProtocol ┌──────────────┐
│ Gazebo ⇄ ros_gz_bridge ⇄ ROS 2 /sim/* topics │  gazebo/* │ gazebo-bridge │ (zenohd)  │ guardian,    │
│                     ⇅                        │ ────────► │ (Rust)        │ ────────► │ cda-sim, …   │
│ bridge/ros_zenoh_bridge.py  (% ⇄ metres)     │ ◄──────── │ mirror/replace│ ◄──────── │              │
└──────────────────────────────────────────────┘           └───────────────┘           └──────────────┘
```

| Zenoh key | Direction | JSON |
|---|---|---|
| `gazebo/window/cmd` | bridge → Gazebo | `{"percent": 25.0}` setpoint, re-sent every second |
| `gazebo/window/position` | Gazebo → bridge | `{"percent": 24.6}` measured glass, ≤ 10 Hz |
| `gazebo/seat/contact` | Gazebo → bridge | `{"contacts": 3}` only while something touches the cushion, ≤ 20 Hz |

**mirror** (`GAZEBO_MODE=mirror`): the bridge listens to `vss_window_state`
and moves the glass. It publishes nothing on uProtocol.

**replace** (`GAZEBO_MODE=replace`): `run.sh` does not start
`window-controller-sim` and `child-presence-sim`. The bridge takes
`uds/window/cmd` and `uds/alarm/cmd` and publishes

- `vss_window_state` from the measured glass position, same JSON as
  `window-controller-sim`: on start, on every command, and while the glass
  moves (whole-percent changes, at most 5 Hz);
- `vss_child_presence` from the seat contact: 300 ms without contact means
  empty, a change must hold 400 ms, plus a heartbeat every second;
  `confidence` 1.0, `zone` `rear_center`. Any object on the cushion counts.

The window mapping lives only in [`config/window.yaml`](config/window.yaml):
`metres = closed_position_m + percent / 100 × travel_m` (0.40 m travel).

## The world

`worlds/cabin.sdf`, primitives only: a car on a parent-and-child parking bay.
Functional parts: the rear-left glass on the prismatic joint
`window_row2_left_joint` (PID-controlled, ~0.15 m/s, a full stroke takes
~3 s, no collision), the row-2 seat cushion with a contact sensor, and the
`child_seat` model that `run.sh seat` moves with `/world/cabin/set_pose`.
Gazebo Fortress (`ign` CLI, `ignition.msgs`) because it is the release
paired with ROS 2 Humble, which `ros2-hvac` uses. `ROS_DOMAIN_ID=42` and
`IGN_PARTITION=guardian_sim` keep it apart from `ros2-hvac` (domain 0).

Inside the container:

```bash
docker compose --profile gazebo exec gazebo-sim bash
ros2 topic echo /sim/joint_states --once
ros2 topic echo /sim/seat/row2/contact     # silent while the seat is empty
ign topic -l
```

## Troubleshooting

| Symptom | Fix |
|---|---|
| `permission denied … docker.sock` | `sudo usermod -aG docker $USER`, then log out and in again |
| Gazebo window black or Ogre errors | `LIBGL_ALWAYS_SOFTWARE=1 ./gazebo-sim/run.sh …` |
| No Gazebo window | Linux with `DISPLAY` only; macOS/Windows run headless (`--no-gui`) |
| `Gazebo did not come up` | `./gazebo-sim/run.sh logs` |
| Closing the Gazebo window stopped the simulation | intended: the launch file stops the container when one process exits; start again |

## Known limitations

- The Guardian never sends "window closed"; use `run.sh window 0` or the
  dashboard button.
- In replace mode do not use the dashboard's "Set Child Present/Absent"
  buttons: they publish on the same topic as the seat sensor. Use
  `run.sh seat` instead. The window buttons are fine.
- Cabin temperature is not simulated in Gazebo; `temperature-sim` stays the
  source.
- Written for `docker compose`; `podman-compose` is untested.

## Files

| Path | Purpose |
|---|---|
| `run.sh` | the one entry point |
| `Dockerfile` | ROS 2 Humble + Gazebo Fortress + eclipse-zenoh image |
| `worlds/cabin.sdf` | the world (`@WINDOW_TRAVEL_M@` filled in at start) |
| `config/window.yaml` | window percent ↔ metres |
| `config/bridge.yaml` | `ros_gz_bridge` topics |
| `config/gui.config` | GUI layout and start camera |
| `launch/gazebo_sim.launch.py` | starts Gazebo, GUI, ros_gz_bridge and the Zenoh bridge |
| `bridge/ros_zenoh_bridge.py` | ROS side of the bridge |
| `compose.gui.yml` | Linux GUI override (added by `run.sh`) |
| `../services/src/bin/gazebo_bridge.rs` | uProtocol side of the bridge |
