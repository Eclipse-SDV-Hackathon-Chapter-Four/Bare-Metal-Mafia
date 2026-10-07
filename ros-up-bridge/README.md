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
  Assisted-by: Anthropic Claude Fable 5.1 (claude-fable-5-1)
-->

# ros-up-bridge — generic ROS 2 ↔ uProtocol bridge

`ros-up-bridge` connects ROS 2 topics to the Guardian stack's Eclipse
uProtocol topics. It is configured with one YAML mapping file per mode and
needs no code generation per message type. Its first user is the Gazebo
cabin simulation ([`gazebo-sim/`](../gazebo-sim/README.md)). It is **our own
work** (Bare-Metal-Mafia), not part of the inherited reference stack.

It is optional: everything is started through compose override files on top
of the `gazebo` profile. `docker-compose.yml` is unchanged, so the default
stack and all existing profiles behave exactly as before. Developer laptops
only, like `gazebo-sim`.

## Architecture

```text
 gazebo-sim container (ROS_DOMAIN_ID 42)                       services (Rust)
┌────────────────────────────────────────┐   plain Zenoh   ┌──────────────────────┐   uProtocol    ┌───────────────┐
│ Gazebo ⇄ ros_gz_bridge ⇄ ROS 2 topics  │   JSON on keys  │ ros_up_mapper        │  over Zenoh    │ guardian,     │
│                    ⇅                   │ ──────────────► │  routes: field maps, │ ─────────────► │ window/child  │
│ ros_zenoh_bridge.py (rclpy, generic)   │ ◄────────────── │  state, presence     │ ◄───────────── │ sims, ...     │
│  rosidl_runtime_py: msg ⇄ JSON         │ ros_up_bridge/* │  up-rust + lib.rs    │  (via zenohd)  │               │
└────────────────────────────────────────┘                 └──────────────────────┘                └───────────────┘
```

- **ROS side**: [`ros_side/ros_zenoh_bridge.py`](ros_side/ros_zenoh_bridge.py),
  a generic rclpy node in the gazebo-sim container. For every `link` it
  converts between one ROS 2 topic and one Zenoh key
  (`<key_prefix>/<link name>`), using `rosidl_runtime_py`
  (`get_message`, `message_to_ordereddict`, `set_message_fields`), so any
  installed message type works. It does not speak uProtocol.
- **uProtocol side**: `ros_up_mapper`
  ([`services/src/bin/ros_up_mapper/`](../services/src/bin/ros_up_mapper/)),
  a Rust binary that maps those Zenoh keys to and from uProtocol URIs. It
  reuses `open_up_transport`, `publish_json_event`, `decode_json_payload`,
  `open_zenoh_session` and the URI functions from `services/src/lib.rs`, so
  uProtocol is spoken only by the same up-rust / up-transport-zenoh stack as
  every other service. `lib.rs` itself is unchanged.
- Both sides read the **same mapping file**; each uses its own section.
- The Python `up-transport-zenoh-python` package is deliberately not used.

## Modes

| Mode | What Gazebo does | Publishes on uProtocol | Config |
|---|---|---|---|
| **mirror** | follows `window-controller-sim`: the rear-left window state drives the Gazebo window joint | **nothing** (enforced when the mapping is loaded) | [`config/mirror.yaml`](config/mirror.yaml), [`compose.mirror.yml`](compose.mirror.yml) |
| **replace** | **replaces** `window-controller-sim` and `child-presence-sim`: window commands move the Gazebo glass, the window state comes from the joint, child presence from the seat contact sensor | `vss_window_state`, `vss_child_presence` | [`config/replace.yaml`](config/replace.yaml), [`compose.replace.yml`](compose.replace.yml) |

### mirror

```text
window-controller-sim ── vss_window_state {window_percentage} ──► ros_up_mapper
   ──► ros_up_bridge/window_position_cmd {data: metres} ──► ros_zenoh_bridge.py
   ──► /sim/window/row2_left/position_cmd ──► Gazebo window joint
```

The existing chain (Guardian → actuation-adapter → cda-sim →
window-controller-sim) is untouched; the bridge only subscribes.

### replace

```text
cda-sim ── uds_window_cmd {percentage} ──► ros_up_mapper ──► Gazebo window joint
cda-sim ── uds_alarm_cmd {enabled} ─────► ros_up_mapper (alarm_enabled)
/sim/joint_states ──► ros_up_mapper ── vss_window_state {window_percentage, alarm_enabled, timestamp_ms}
/sim/seat/row2/contact ──► ros_up_mapper ── vss_child_presence {present, confidence, zone, timestamp_ms}
```

- **Window**: the mapper consumes exactly what `window-controller-sim`
  consumed (`uds/window/cmd`, `uds/alarm/cmd`) and publishes the window state
  with the same JSON schema (`WindowStateEvent`). Like `window-controller-sim`
  it publishes on start and on every window or alarm command, and it serves
  `GET /state` (`{window_percentage, alarm_enabled}`) and `GET /health` on
  port 8092. There are no acks or RPC replies to imitate:
  `window-controller-sim` never sent any.
  **Difference**: `window_percentage` is the *measured* joint position, so
  while the glass moves the state is also published in between. Those
  intermediate events are throttled: only on a change of at least 1
  percentage point, at most 5 Hz, and the final value is always published
  (a change held back by the rate limit goes out as soon as allowed). A
  window command therefore first reports the old position and then follows
  the glass. All of this is configured in `replace.yaml`
  (`publish: on_change / max_rate_hz / coalesce_ms`).
- **Child presence**: Gazebo publishes contacts only while something touches
  the row-2 cushion (~1 kHz; the ROS side forwards at most 20 Hz). No
  contact message for `silence_ms` (300 ms) means "empty"; a change must be
  stable for `debounce_ms` (400 ms, configurable) before an event is
  published, plus a heartbeat every `heartbeat_ms` (1 s). `confidence` is
  fixed at **1.0**: the simulated contact sensor delivers no confidence of its
  own, and in simulation the contact is ground truth (the simulator used
  0.97–0.99). `zone` is `rear_center`, as in `child-presence-sim`, and the
  child seat sits in the middle of the row-2 bench. Any object on the
  cushion counts as "present".
- **Switching**: `compose.replace.yml` moves `window-controller-sim` and
  `child-presence-sim` into the profile `replaced-by-gazebo`, which is never
  activated, and drops them from the `depends_on` lists of `temperature-sim`
  and `cda-sim` with `!override`. **This needs Docker Compose ≥ 2.24 and
  probably does not work with `podman-compose`.** The default stack and
  every profile are unchanged.
- **The Guardian does not close the window**: when the child is removed the
  Guardian goes back to `CLEAR` but sends no "close window" command, so the
  Gazebo glass stays open. This is existing Guardian behaviour and
  deliberately not changed.

## Start

Commands use `docker compose`; with Podman write `podman-compose`. From the
repository root:

```bash
# mirror, headless
docker compose -f docker-compose.yml -f ros-up-bridge/compose.mirror.yml --profile gazebo up --build

# mirror with the Gazebo GUI (Linux only, see gazebo-sim/README.md)
xhost +local:
docker compose -f docker-compose.yml -f ros-up-bridge/compose.mirror.yml \
  -f gazebo-sim/compose.gui.yml --profile gazebo up --build

# replace, headless (add -f gazebo-sim/compose.gui.yml for the GUI)
docker compose -f docker-compose.yml -f ros-up-bridge/compose.replace.yml --profile gazebo up --build
COMPOSE_CMD="docker compose" ./gazebo-sim/child-seat.sh place    # child present -> Guardian MONITORING
COMPOSE_CMD="docker compose" ./gazebo-sim/child-seat.sh remove
```

In replace mode `ros-up-mapper` serves `/health`, `/stats` and `/state` on
port 8092 (the port of the replaced `window-controller-sim`).

`ros-up-mapper` serves `GET /health` and `GET /stats` (counts per route,
`uprotocol_published`, `zenoh_published`) on port 8096 in mirror mode.

A mirror only sees window states published **after** it started
(uProtocol pub/sub has no retained last value), and `window-controller-sim`
publishes only on change. The mirror route therefore re-sends its last
setpoint every second (`repeat_last_ms: 1000`), so a lost message is
corrected within a second; a mapper started after the last window change
still waits for the next one.

## Mapping format

One YAML file per mode, read by both sides. `${name.key}` inserts a value
from a `constants` file; a string that is exactly one reference keeps the
constant's type (a number stays a number).

```yaml
version: 1
mode: mirror                 # mirror | replace; mirror may not publish on uProtocol

zenoh:
  key_prefix: ros_up_bridge  # Zenoh key of a link: <key_prefix>/<link name>

constants:                   # loaded from other files
  window:
    file: /opt/gazebo_sim/config/window.yaml
    path: window_row2_left   # -> ${window.travel_m}, ${window.joint_name}, ...

links:                       # ROS 2 <-> Zenoh, read by ros_zenoh_bridge.py
  - name: window_position_cmd
    direction: to_ros        # to_ros | from_ros
    ros: {topic: /sim/window/row2_left/position_cmd, type: std_msgs/msg/Float64}
    # max_rate_hz: 10        # from_ros only: forward the latest message at most
                             # this often; the last one of a burst always goes out

routes:                      # Zenoh <-> uProtocol, read by ros_up_mapper
  - name: mirror_window_position
    kind: forward            # one output message per input message
    from: {uprotocol: vss_window_state}
    to: {link: window_position_cmd}
    repeat_last_ms: 1000     # optional, link outputs only: re-send the last output
    fields:                  # output field -> FieldSpec
      data:
        from: window_percentage
        ops: [{div: 100}, {mul: "${window.travel_m}"}, {add: "${window.closed_position_m}"}]
        type: f64

http:
  port: 8096                 # /health, /stats
```

**Route kinds**

| `kind` | Keys | Behaviour |
|---|---|---|
| `forward` | `from`, `to`, `fields`, `repeat_last_ms` | one output per input message |
| `state` | `inputs`, `initial`, `publish`, `to`, `fields`, `http` | keeps a state object; each input has a `name`, `from`, optional `set:` (state fields from that input, FieldSpecs) and `publish: true` (publish whenever it arrives). `publish:` has `on_start`, `on_change` (field → `{min_delta}`; publish when it differs that much from the last published value), `max_rate_hz` (a held-back change is published as soon as allowed, so the final value always goes out) and `coalesce_ms` (a burst becomes one event). Output `fields` read the state with `{state: <field>}`. `http: {path, fields}` serves the state on the mapper's HTTP port |
| `presence` | `from`, `silence_ms`, `debounce_ms`, `heartbeat_ms`, `to`, `fields` | for "activity" streams (messages only while something is detected): no input for `silence_ms` → absent; a change must last `debounce_ms`; published on change plus every `heartbeat_ms`; `{state: present}` is the boolean |

**`repeat_last_ms`** re-sends the last output periodically. It is allowed
only for outputs to a ROS link (an idempotent setpoint); on uProtocol a
repeat would be a duplicate event, so the mapper rejects it there.

**Endpoints** (`from`, `to`): `{link: <name>}` (must exist in `links` with
the matching direction) or `{uprotocol: <name>}`, where `<name>` is a URI
function from `services/src/lib.rs` without the `_uri` suffix
(`vss_window_state`, `vss_child_presence`, `uds_window_cmd`,
`uds_alarm_cmd`, ...), so the IDs stay defined in one place. Explicit
`{uprotocol: {authority, ue_id, version, resource}}` also works.

**FieldSpec**: exactly one source, then `ops` in order, then `type`.

| Key | Meaning |
|---|---|
| `from: <path>` | value from the input message. Paths: `a.b`, `a[2]`, `position[name==joint]` (element of `position` at the index where the sibling array `name` equals `joint`, the `JointState` layout) |
| `const: <value>` | a constant (renaming = `from` under a new output name) |
| `now_ms: true` | current time in ms since the Unix epoch (timestamps) |
| `ops: [...]` | `{add: x}`, `{sub: x}`, `{mul: x}`, `{div: x}` (scaling), `round`, `{clamp: [min, max]}` |
| `type:` | `f64`, `i64`, `u8`, `u64`, `bool`, `string` |

Dotted output names (`header.frame_id`) create nested objects.

**Window percent ↔ metres** is not defined here: the routes use
`${window.travel_m}` and `${window.closed_position_m}` from
[`gazebo-sim/config/window.yaml`](../gazebo-sim/config/window.yaml), the one
place that mapping lives. Both images contain that file at
`/opt/gazebo_sim/config/window.yaml`.

## Never two publishers

`TOPIC_WINDOW_STATE` (`vss_window_state`) and `TOPIC_CHILD_PRESENCE`
(`vss_child_presence`) must never have two publishers.
[`check-single-publisher.sh`](check-single-publisher.sh) renders the
effective compose configuration for the given files and profiles and counts
publishers per topic: `window-controller-sim`, `child-presence-sim`, and every
uProtocol output in the mapping file each `ros-up-mapper` service is started
with. More than one publisher → `FAIL`, exit code 1.

```bash
COMPOSE_CMD="docker compose" ./ros-up-bridge/check-single-publisher.sh \
  -f docker-compose.yml -f ros-up-bridge/compose.mirror.yml --profile gazebo
```

In mirror mode the mapper has no uProtocol outputs at all.

**At runtime** uProtocol publish events carry the topic as their source, not
the publisher, so a second publisher cannot be recognised by its address.
`ros_up_mapper` therefore subscribes to every topic it publishes and keeps
a hash of each payload it sent; any other payload on that topic came from a
second publisher. By default it logs an error (`SECOND PUBLISHER on ...`),
reports `GET /health` as `503 unhealthy`, counts the event in `/stats`
(`foreign_publisher_events`) and keeps running. With
`ROS_UP_STRICT_SINGLE_PUBLISHER=1` it exits with code 1 instead; the tests
use the strict mode.

## Tests

```bash
COMPOSE_CMD="docker compose" ./ros-up-bridge/test-mirror.sh          # headless
COMPOSE_CMD="docker compose" GUI=1 ./ros-up-bridge/test-mirror.sh    # with GUI (Linux, xhost)
COMPOSE_CMD="docker compose" BREAK=mapper ./ros-up-bridge/test-mirror.sh   # negative check, must FAIL
```

`test-mirror.sh` (re)starts the stack itself: first `zenohd`, `gazebo-sim`
and `ros-up-mapper`, then the default stack, whose scripted scenario
escalates the Guardian to `MITIGATING` with the window at 25 %. It checks the
static single-publisher check, that the Guardian reaches `MITIGATING` and
`window-controller-sim` reports 25 %, that the Gazebo joint reaches the
window state **last published on uProtocol** (normally 25 % = 0.10 m) within
±5 mm (observed read-only, `sim_check.py joint`), and that the mapper
published 0 uProtocol messages with no route errors. The bus state is read
from the mapper's `/stats` (`routes.<name>.last_input`); if it differs from
what `window-controller-sim` reports over HTTP, the test prints a `WARN`
line naming the publish race described below. `KEEP=1` leaves the stack
running, `BUILD=1` rebuilds.

```bash
COMPOSE_CMD="docker compose" ./ros-up-bridge/test-replace.sh                  # headless
COMPOSE_CMD="docker compose" GUI=1 ./ros-up-bridge/test-replace.sh            # with GUI
COMPOSE_CMD="docker compose" BREAK=foreign ./ros-up-bridge/test-replace.sh    # negative check, must FAIL
```

`test-replace.sh` (re)starts the stack with the mapper in strict mode and
holds `temperature-sim` back so the Guardian's states come in a fixed order:

1. static check: PASS for replace, and FAIL (as expected) when the replaced
   simulators are re-enabled with `--profile replaced-by-gazebo`
2. `window-controller-sim` and `child-presence-sim` are not running; empty
   seat → Guardian `CLEAR`
3. child seat placed in Gazebo → `MONITORING`
4. `temperature-sim` started → `WARNING` → `CRITICAL` → `MITIGATING`
5. stage-2 mitigation: the mapper's `/state` reports 25 % (from the joint),
   the Gazebo joint is at 0.10 m ±5 mm, `alarm_enabled` is true, and the
   dashboard (a uProtocol subscriber) shows 25 %
6. the window state events while the glass moved (read from the mapper's
   debug log): intermediate values, ≥ 1 point and ≥ 200 ms apart, final 25 %
7. child seat removed → `CLEAR`
8. one publisher per topic at runtime: the replaced simulators are not
   running, the strict mapper is still running, `foreign_publisher_events`
   is 0, `/health` is ok

`BREAK=foreign` starts `window-controller-sim` as a second publisher; the
strict mapper exits on its first message and the test fails with exit
code 1.

Unit tests of the mapping engine: `cargo test --bin ros_up_mapper` (11
tests, including loading both shipped mapping files).

## Versions

| Component | Version | Pinned by |
|---|---|---|
| `zenoh` crate (mapper, all Rust services) | 1.9.0 | `Cargo.lock`, built with `--locked` |
| `eclipse-zenoh` (Python, ROS side) | 1.9.0 | `pip3 install eclipse-zenoh==1.9.0` in `gazebo-sim/Dockerfile` |
| `up-rust` / `up-transport-zenoh` | 0.9.0 / 0.9.1 | `Cargo.lock` |
| Zenoh router `eclipse/zenoh` | `:latest`, **1.10.1** at the time of testing | not pinned |

**Risk:** `docker-compose.yml` runs the router as `eclipse/zenoh:latest`
(not changed here, it belongs to the default stack). The clients are 1.9.0;
the tests passed against router 1.10.1. Zenoh 1.x aims at protocol
compatibility within 1.x, but a future `:latest` (for example a 2.x) could
break all clients at once. Pinning the router to `eclipse/zenoh:1.9.0`
(the tag exists) would remove the risk; that is a team decision because it
changes the default stack.

## Found while testing: publish race in window-controller-sim

`window-controller-sim` (reference stack) handles the window command and the
alarm command in two listeners. Both used to take a state snapshot, release
the lock and then publish concurrently. When the Guardian's stage-2
mitigation sends "window 25 %" and "alarm on" together, the older snapshot
(`{window 0 %, alarm on}`) could be the last state on the bus. Every
subscriber then kept the stale 0 %: Gazebo (correctly mirroring the bus) and
also `temperature-sim`, which therefore did not cool the cabin, while
`window-controller-sim` itself reported 25 % over HTTP. It showed up in
roughly one of six mirror runs, more often under load (GUI).

Root cause: `up-transport-zenoh` 0.9.1 runs every listener call in its own
tokio task, so the two commands and also the two resulting state events can
be handled in either order; a publisher-side lock alone did not help (a
throwaway stress test still saw 88 of 200 rounds end stale).

The fix is a separate commit ("Fix stale window state race in
window_controller_sim"): the listeners only update the state and wake one
publisher task, which waits 20 ms to coalesce changes and then publishes a
single event with the final state. Stress test (window and alarm command
sent at the same moment, last received state checked): **24 of 100** rounds
stale before, **0 of 200** after. It touches an existing service, so it is
kept separate and can be reviewed or dropped on its own; side effect: a
burst of commands within 20 ms yields one state event instead of one per
command. Without the fix, `test-mirror.sh` still passes (Gazebo matches the
bus) but may print the `WARN` line.

## Known limitations

- **Late join in mirror mode**: no retained state on uProtocol pub/sub. A
  mapper that starts after the last window change waits for the next one;
  lost messages are corrected by `repeat_last_ms`.
- **`serde_yaml` is no longer maintained** (0.9.34, marked deprecated
  upstream). It was chosen because it is already in `Cargo.lock` through
  Zenoh, so the mapper adds no new crate. Replace it if Zenoh drops it.
- **Rare Guardian stall in the mirror test (not explained yet)**: in 2 of
  about 32 `test-mirror.sh` runs the Guardian stayed at `MONITORING` and the
  default scenario never escalated. The mirror publishes nothing on
  uProtocol (`uprotocol_published = 0`, checked in every run), and 12 runs
  of the plain default stack without Gazebo all reached `MITIGATING`; the
  extra CPU load of Gazebo is a plausible but unproven factor. On this
  failure the test now prints the last log lines of `guardian`,
  `temperature-sim` and `child-presence-sim`.
- **Delivery order on uProtocol**: `up-transport-zenoh` 0.9.1 runs every
  listener call in its own tokio task, so two events published within a
  few milliseconds can be processed in either order by any subscriber, and
  uProtocol message IDs only order to the millisecond. Publishers of state
  should not publish bursts (see the window-controller-sim fix above).
- **Compose and Podman**: tested with Docker Compose v5 only; not run under
  `podman-compose`. `compose.replace.yml` uses `!override` (Compose ≥ 2.24),
  which `podman-compose` probably does not support.
- **Window state in replace mode** reflects the measured glass position, so
  a command is first answered with the old position (see "replace").
- **The Guardian does not close the window**: when it returns to `CLEAR` it
  sends no "window closed" command, so the window (and the Gazebo glass)
  stays where the last mitigation put it. This is existing Guardian
  behaviour and deliberately not changed.

## Files

| Path | Purpose |
|---|---|
| `ros_side/ros_zenoh_bridge.py` | ROS side (in the gazebo-sim image) |
| `../services/src/bin/ros_up_mapper/` | uProtocol side (Rust) |
| `Dockerfile` | `hack-to-the-future/ros-up-mapper:stage-bridge` |
| `config/mirror.yaml`, `config/replace.yaml` | mappings |
| `compose.mirror.yml`, `compose.replace.yml` | compose overrides |
| `check-single-publisher.sh` | static single-publisher check |
| `test-mirror.sh`, `test-replace.sh` | end-to-end tests |
