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

# gazebo-sim — optional Gazebo simulation backend

`gazebo-sim` adds a physics simulation of the vehicle cabin to the Guardian
Loop stack: a rear-left window that actually moves, and a rear seat with a
contact sensor for later child-presence work. It is **our own work**
(Bare-Metal-Mafia), not part of the inherited reference stack.

It is **optional** and lives only in the compose profile `gazebo`. The default
stack (`docker compose up`) is unchanged. It runs on **developer laptops
only** and is not part of any Raspberry Pi / AutoSD HPC deployment.

This is step 1: Gazebo, a minimal world and `ros_gz_bridge`, tested. The
generic ROS 2 ↔ uProtocol bridge that connects it to the Guardian comes
later; for now nothing here talks to Zenoh.

## Why Gazebo Fortress

The rest of the stack (`ros2-hvac/`) is on **ROS 2 Humble**, and the Gazebo
release officially paired with Humble is **Fortress** (ign-gazebo 6). The
image installs `ros-humble-ros-gz-sim`, `-bridge` and `-interfaces` from the
ROS apt repository, which pull Fortress (`libignition-gazebo6`,
`ignition-msgs8`, `ignition-transport11`). We chose it deliberately so ROS
and Gazebo come from one consistent package set: no Harmonic, no Gazebo
Classic, no mixed versions.

Consequences you will notice: the CLI is `ign` (not `gz`), message types
are `ignition.msgs.*`, plugins are `ignition-gazebo-*-system`, and the
partition variable is `IGN_PARTITION`.

## Start

Commands use `docker compose`; with Podman write `podman-compose`. Run them
from the repository root.

### Headless (default, all host OSes)

```bash
docker compose --profile gazebo up --build -d gazebo-sim   # only the simulation
docker compose --profile gazebo up --build                 # default stack + simulation
```

The container runs `ign gazebo -s -r` (server only, simulation running) and
`ros_gz_bridge`, both from `launch/gazebo_sim.launch.py`. The first build
takes a few minutes (see [Image size and build time](#image-size-and-build-time)).

### With GUI (Linux hosts only)

```bash
xhost +local:
docker compose -f docker-compose.yml -f gazebo-sim/compose.gui.yml --profile gazebo up --build gazebo-sim
```

`compose.gui.yml` sets `GZ_GUI=true`, forwards `DISPLAY` and
`/tmp/.X11-unix`, and passes `/dev/dri` for GPU acceleration. With
`GZ_GUI=true` the launch file runs `ign gazebo -r` (server **and** GUI in one
process; `ign gazebo -g` alone would only start a client with no server)
with [`config/gui.config`](config/gui.config): the camera starts outside the
car, above the rear-left door, so you see the window face-on and look into
the cabin from above (the cabin has no roof visual for that reason). Orbit
with the mouse; the camera button (or service `/gui/screenshot`) saves a PNG
inside the container. Closing the GUI window stops the simulation and the
container, because both run in one process.

Then run the guided demo ([Demo](#demo)) and watch it in that window.

If the GUI window stays black or Ogre fails to start (no usable GPU, a VM,
NVIDIA without the container toolkit), fall back to software rendering:

```bash
LIBGL_ALWAYS_SOFTWARE=1 docker compose -f docker-compose.yml -f gazebo-sim/compose.gui.yml --profile gazebo up gazebo-sim
```

Run `xhost -local:` afterwards if you do not want to keep X access open.

**macOS and Windows: headless only.** The GUI override needs a Linux X
server socket and `/dev/dri`. Docker Desktop on macOS/Windows has neither.
Use the topics and the test script instead. WSLg may work but is untested.

## What is in the world

`worlds/cabin.sdf`, primitives only (no meshes, no Fuel downloads):

| Entity | Notes |
|---|---|
| `ground_plane` | static |
| `cabin` / `body` | floor, roof (collision only, invisible), walls, rear-left door with a window opening, welded to the world |
| `cabin` / `seat_row2` | rear seat (base, cushion, backrest); **contact sensor** on the cushion collision |
| `cabin` / `window_row2_left` | glass on the prismatic joint `window_row2_left_joint`, axis pointing down, `<gravity>false</gravity>`, visual only (no collision) |
| `child_seat` | a toddler (primitives: head, torso, arms, legs) in an orange child seat; one free rigid body, only the seat base and backrest collide. Starts on the ground beside the rear-left door |

World systems: Physics, UserCommands (for `set_pose`), SceneBroadcaster (for
the GUI), Contact. Model systems: JointPositionController and
JointStatePublisher on the window joint, both with explicit `<topic>`.
There is deliberately **no Sensors system**: it needs a rendering engine,
which is unstable headless without a GPU, and the contact sensor does not
need it.

## Topics

ROS 2 side, `ROS_DOMAIN_ID=42`. Bridge configuration:
[`config/bridge.yaml`](config/bridge.yaml).

| ROS 2 topic | ROS 2 type | Direction | Gazebo topic | Gazebo type |
|---|---|:--:|---|---|
| `/sim/window/row2_left/position_cmd` | `std_msgs/msg/Float64` (metres) | ROS → GZ | `/model/cabin/joint/window_row2_left_joint/cmd_pos` | `ignition.msgs.Double` |
| `/sim/joint_states` | `sensor_msgs/msg/JointState` | GZ → ROS | `/model/cabin/joint_state` | `ignition.msgs.Model` |
| `/sim/seat/row2/contact` | `ros_gz_interfaces/msg/Contacts` | GZ → ROS | `/model/cabin/seat/row2/contact` | `ignition.msgs.Contacts` |
| `/clock` | `rosgraph_msgs/msg/Clock` | GZ → ROS | `/clock` | `ignition.msgs.Clock` |

Gazebo publishes contacts **only while something touches the cushion**. An
empty seat means no messages at all, not an empty message.

### Domain and partition isolation

`ros2-hvac` uses `ROS_DOMAIN_ID=0`. `gazebo-sim` sets `ROS_DOMAIN_ID=42`
(in the Dockerfile and, visibly, in `docker-compose.yml`), so the two ROS
graphs never see each other even though they share the compose network.
`IGN_PARTITION=guardian_sim` does the same for Gazebo Transport. Any tool
that wants to see `/sim/*` must use domain 42; running inside the container
(`docker compose exec gazebo-sim bash`) does that automatically.

## Window mapping, 0–100 %

Defined in one place, [`config/window.yaml`](config/window.yaml):

```text
joint_position_m = closed_position_m + (percent / 100) * travel_m
                 = 0.0               + (percent / 100) * 0.40
```

| Opening | Joint position |
|---:|---:|
| 0 % (closed) | 0.00 m |
| 25 % | 0.10 m |
| 100 % (open) | 0.40 m |

The launch file writes `travel_m` into the joint's upper limit
(`@WINDOW_TRAVEL_M@` in `cabin.sdf`), and `tools/sim_check.py` converts
percent with the same file. `/sim/window/row2_left/position_cmd` itself
carries **metres**; whoever speaks percent (the later uProtocol bridge,
following the window controller's `window_percentage`) must apply this
mapping. `travel_m` may not exceed 0.40 m, the height of the opening.

## Demo

```bash
COMPOSE_CMD="docker compose" ./gazebo-sim/demo.sh
```

Plays a short story with pauses (`DEMO_PAUSE_S`, default 3 s), best watched
in the GUI: child seat outside → child placed on the rear seat (contact
sensor fires) → window 25 % (Guardian stage 2) → 100 % → closed → child
taken out (contacts stop). Each step prints what the ROS 2 side measured.
Takes about a minute.

## Try it yourself

```bash
docker compose --profile gazebo exec gazebo-sim bash
source /opt/ros/humble/setup.bash

ros2 topic pub --once /sim/window/row2_left/position_cmd std_msgs/msg/Float64 "{data: 0.10}"   # 25 %
ros2 topic echo /sim/joint_states --once
ros2 topic echo /sim/seat/row2/contact          # silent until a child seat is placed
ign topic -l                                    # Gazebo-side topics
```

Child seat test object, from the host:

```bash
COMPOSE_CMD="docker compose" ./gazebo-sim/child-seat.sh place    # onto the row 2 cushion
COMPOSE_CMD="docker compose" ./gazebo-sim/child-seat.sh remove   # back onto the ground outside
```

`child-seat.sh` calls the Fortress UserCommands service
`/world/cabin/set_pose`; the child seat drops 1 cm onto the cushion and
rests there. In the GUI you can also drag it with the transform tool.

## Smoke test

```bash
docker compose --profile gazebo up --build -d
COMPOSE_CMD="docker compose" ./gazebo-sim/test-window.sh
```

Same style as `ros2-hvac/test-can-e2e.sh`: `COMPOSE_CMD` selects the compose
command (default `podman-compose`), every check prints `PASS`/`FAIL`, and the
script exits 1 if any check fails. It checks:

1. container running, `ROS_DOMAIN_ID=42`
2. Gazebo world up with the explicit topic names
3. bridged ROS 2 topics exist, `/clock` advances
4. window setpoints 25 %, 100 % and 0 % reach the mapped joint position on
   `/sim/joint_states` within ±5 mm (`TOLERANCE_M` overrides) and stay there
   for 0.5 s
5. seat contact: no messages with an empty seat, `child_seat` reported after
   `place`, no messages again within 3 s after `remove`

## Files

| Path | Purpose |
|---|---|
| `Dockerfile` | ROS 2 Humble + Gazebo Fortress image `hack-to-the-future/gazebo-sim:stage-gazebo` |
| `worlds/cabin.sdf` | the world (template: `@WINDOW_TRAVEL_M@`) |
| `config/window.yaml` | window percent ↔ joint position mapping |
| `config/bridge.yaml` | `ros_gz_bridge` topic configuration |
| `launch/gazebo_sim.launch.py` | renders the world, starts Gazebo and the bridge; stops both if one exits |
| `compose.gui.yml` | Linux-only GUI override |
| `config/gui.config` | GUI layout and start camera (GUI mode only) |
| `demo.sh` | guided demo, watch it in the GUI |
| `child-seat.sh` | place / remove the child seat test object |
| `tools/sim_check.py` | ROS 2 checks used by the smoke test |
| `test-window.sh` | smoke test |

## Image size and build time

Measured on a 4-core / 6 GB Linux laptop (Docker 29):

| | |
|---|---|
| Image `hack-to-the-future/gazebo-sim:stage-gazebo` | **2.9 GB** (base `ros:humble-ros-base` 1.17 GB; the rest is Gazebo Fortress with its Ogre/Qt/FFmpeg dependencies). For comparison, `ros2-hvac` is 4.0 GB |
| Build, `--no-cache`, base image already pulled | **~5 min** (295 s), a single apt layer, so mostly download speed |
| Runtime, headless | ~1 CPU core (1 ms physics steps in real time), ~110 MB RAM |
| Runtime, with GUI | ~2+ CPU cores, ~370 MB RAM |

No Rust build is involved, so this image does not slow down the default
stack's build.

## Known limitations

- **Tested with Docker only.** Verified with Docker 29 / Compose v5 on
  Linux, headless and with GUI (XWayland, both with `/dev/dri` and with
  `LIBGL_ALWAYS_SOFTWARE=1`). The scripts follow the repo's `podman-compose`
  convention but have not been run under Podman yet.
- **Headless only on macOS/Windows.** The GUI override needs X11 and
  `/dev/dri` on a Linux host.
- **Not wired into the Guardian yet.** No Zenoh / uProtocol connection;
  that is the job of the upcoming ROS 2 ↔ uProtocol bridge.
- **Window glass has no collision.** It cannot pinch or be blocked; this
  avoids fighting the door frame and keeps the controller simple.
- **Simplified dynamics.** The window is position-controlled by a PID
  (`p=200`, `d=30`) with gravity off on the glass. Joint damping (40 N·s/m)
  and a force limit (`cmd_max` 6 N) cap it at roughly window-motor speed,
  ~0.15 m/s (a full stroke takes about 3 s); it settles within ~2 mm of the
  target and has no end-stop behaviour beyond the joint limits.
- **Contact sensor is binary in practice.** It reports touching collisions,
  not weight or occupant class.
- **Contact messages arrive at physics rate.** Fortress' Contact system
  ignores `<update_rate>` and publishes every physics step (~1 kHz measured
  on `/sim/seat/row2/contact`) while the child seat rests on the cushion. The
  later uProtocol bridge must debounce/throttle this into presence events.
- **`/sim/window/.../position_cmd` takes metres, not percent.** Percent
  conversion belongs to the caller (see the mapping above).

## Open points

- ROS 2 ↔ uProtocol bridge: map the window controller's `window_percentage`
  to `position_cmd`, publish `/sim/joint_states` back as window state, and
  turn `/sim/seat/row2/contact` into a child-presence VSS event.
- Decide whether Gazebo should become the window actuator (replacing
  `window-controller-sim`) or only mirror it.
- Cabin temperature is not simulated in Gazebo; `temperature-sim` stays the
  source.
- Spawning or deleting the child seat (instead of moving one fixed model) via
  `/world/cabin/create` and `/world/cabin/remove` if multiple occupants are
  needed.
- GUI on WSLg is untested.
