<div align="center">

# Bare-Metal-Mafia

### Guardian Loop — portable child presence detection

*Eclipse SDV Hackathon 2026 · [Hack to the Future](https://github.com/Eclipse-SDV-Hackathon-Chapter-Four/Hack-to-the-Future) challenge*

[What we achieved, and what we did not](docs/Achieved.md)

</div>

---

## Goal

Build a **Child Presence Detection and Mitigation** feature that:

- detects a child in the car,
- monitors the cabin temperature,
- decides how dangerous the situation is,
- warns and acts (air conditioning, windows, alarm).

The feature must keep working unchanged while the hardware underneath it is swapped.

> ### The Golden Rule
>
> **The Guardian Loop logic must not change between simulation and real hardware.**
>
> The Guardian therefore only uses uProtocol service interfaces:
>
> | Forbidden inside Guardian | Use instead |
> | --- | --- |
> | CAN, GPIO, serial ports | uProtocol pub/sub on VSS topics |
> | UDS, DoIP, ECU DIDs | uProtocol RPC to the Actuation Adapter, then CDA |
> | SOME/IP | a SOME/IP to uProtocol bridge service |
> | Hardware addresses, device paths | service URIs only |
>
> If swapping a sensor or an actuator forces a change to `guardian.rs`, the design is wrong.

---

## Task Distribution

| # | Member | Task | Note |
| :--: | --- | --- | --- |
| 1 | Elias | Overview, ROS 2 > Gazebo simulation | Git, backlog |
| 2 | Lars | Overview, management | Git, AutoSD (openDuT not used) |
| 3 | Terra | openDuT, OpenBSW | Working hardware |
| 4 | Dimitri | openDuT, OpenBSW | Working hardware |
| 5 | Katharina | Loop features | Software architecture > Guardian loop API |

---

## Architecture

```mermaid
flowchart LR
    subgraph SENSE["Sense"]
        CPS["Child Presence<br/>(sim)"]
        TMP["Temperature<br/>(sim / AZ3166)"]
    end

    subgraph HPC["AutoSD HPC"]
        GL["<b>Guardian Loop</b><br/>CLEAR → MONITORING →<br/>WARNING → CRITICAL →<br/>MITIGATING"]
        AA["Actuation Adapter"]
    end

    subgraph ACT["Actuate"]
        CDA["OpenSOVD CDA"]
        WIN["OpenBSW<br/>Window Controller"]
        HVAC["ROS 2 HVAC<br/>(Eclipse Muto)"]
    end

    CPS -->|"pub/sub"| GL
    TMP -->|"pub/sub via SOME/IP bridge"| GL
    GL -->|"RPC"| AA
    AA --> CDA
    AA --> HVAC
    CDA --> WIN
```

**Transport:** every uProtocol message passes through an Eclipse Zenoh router.
No service knows where any other service runs, and that is what makes the swap possible.

### Communication patterns

| Pattern | Use it for | Example |
| --- | --- | --- |
| **uProtocol pub/sub** | sensor data and state broadcasts: fire and forget, many listeners | temperature event, child-presence event, Guardian state |
| **uProtocol RPC** | one service asking another to perform an operation and awaiting the result | Guardian to Actuation Adapter: HVAC on, 18 °C, fan 100 %, close window |

For other systems the Guardian also offers an Early Warning System (EWS) WebSocket API,
used by the Telegram notification service. See [notification/README.md](notification/README.md).

---

## Development Journey

The official challenge progression, and where we ended up.
The full recap, including what we did not reach, is in [docs/Achieved.md](docs/Achieved.md).

| Stage | Objective | Status | Notes |
| :--: | --- | --- | --- |
| **1** | **Guardian Loop on your laptop.** Simulated sensors publish over uProtocol, Guardian shows state transitions | **Done** | Inherited from the reference stack. `docker compose up` shows the full escalation within seconds |
| **2** | **Add simulated actuation.** uProtocol RPC to Actuation Adapter, CDA, window controller | **Done** | The SIL loop is closed end to end. The ROS 2 HVAC path is wired up as well |
| **3** | **Run Guardian on AutoSD.** Same artifact, only deployment and configuration change | **Partial** | Guardian and zenohd run as real Podman containers inside AutoSD on a Raspberry Pi. The rest of the stack was never built in the VM. See [deploy/AUTOSD_ON_PI.md](deploy/AUTOSD_ON_PI.md) |
| **4** | **Replace the temperature simulator.** AZ3166 with Eclipse ThreadX over SOME/IP | **Done** | Runs on the physical board over USB serial, plus the Renode and SOME/IP paths |
| **5** | **Replace the simulated actuator.** openDuT switches to OpenBSW or physical targets | **Partial** | The S32K148 with OpenBSW is driven over real DoIP/UDS. openDuT itself was never started |

---

## Building Blocks

| Block | Location | Status |
| --- | --- | --- |
| **Guardian Loop**, hazard state machine | `services/src/bin/guardian.rs`, logic in `services/src/lib.rs` | Done |
| **Child Presence Sensor**, simulated | `services/src/bin/child_presence_sim.rs` | Done, simulated |
| **Temperature Sensor**, simulated with closed-loop thermal model | `services/src/bin/temperature_sim.rs` | Done, simulated |
| **Temperature Sensor**, ThreadX firmware | `threadx-temp-sensor/` (Renode) | Partial, emulated |
| **Temperature Sensor**, AZ3166 hardware | `az3166-sensor-bridge-firmware/`, `services/src/bin/az3166_serial_bridge.rs` | Done, **on real hardware** (LSM6DSL die temperature + HTS221 humidity over USB serial) |
| **SOME/IP to uProtocol bridges** | `someip_uprot_bridge.rs`, `someip_window_bridge.rs` | Done |
| **Actuation Adapter**, uProtocol RPC to diagnostics | `services/src/bin/actuation_adapter.rs` | Done |
| **OpenSOVD CDA**, diagnostic bridge | `services/src/bin/cda_sim.rs` | Done, simulated |
| **OpenBSW Window Controller**, simulated | `services/src/bin/window_controller_sim.rs` | Done, simulated |
| **OpenBSW Window Controller**, S32K148 hardware | `firmware/`, `services/src/bin/s32k148_doip_bridge.rs` | Done, **on real hardware** (UDS `0x2E` over DoIP, DID `0xCF20`) |
| **ROS 2 HVAC workload**, Eclipse Muto with CAN bridge | `ros2-hvac/`, `services/src/bin/ros2_hvac_bridge.rs` | Done |
| **Dashboard**, live one-page view | `services/src/bin/dashboard.rs`, port 8094 | Done |
| **Gazebo cabin simulation**, Fortress world (window joint, seat contact) with a ROS 2 ↔ uProtocol bridge; mode *mirror* (Gazebo follows the window state) and *replace* (Gazebo replaces the window controller and child presence simulators), one start script, laptops only | `gazebo-sim/`, `services/src/bin/gazebo_bridge.rs` | Done, **our own work** (not from the reference stack) |
| **AutoSD HPC deployment** | `deploy/AUTOSD_ON_PI.md`, `deploy/setup-raspi-guardian-node.sh` | Partial, Guardian + zenohd proven in the VM |
| **eCall / Notification service**, Telegram via EWS WebSocket (Java) | [`notification/`](notification/README.md) | Done, mock |

> Most blocks come from the reference stack: the challenge is integration and portability
> rather than reimplementation. The rows marked *our own work* are the exception.

---

## Quick Start

```bash
docker compose up --build          # full SIL stack
```

Then open the dashboard at <http://localhost:8094>.

The simulators run a short scenario at start-up. The cabin starts at 26 °C and heats up
by about 0.5 °C per second until something cools it:

| Time | Event | Guardian state |
| :--: | --- | --- |
| ~1 s | no child | `CLEAR` |
| ~4 s | child present, confidence 0.98, cabin already above 25 °C | `WARNING` |
| ~5 s | 28.5 °C | `CRITICAL`, then `MITIGATING`: HVAC on, 18 °C, fan 100 % |
| +12 s | still ≥ 28.5 °C, or HVAC fault injected on the dashboard | `MITIGATING` stage 2: window open, alarm |

Optional profiles:

```bash
docker compose --profile ros2 up --build      # adds ROS 2 HVAC via Eclipse Muto
docker compose --profile threadx up --build   # adds the ThreadX sensor over SOME/IP
./gazebo-sim/run.sh demo                      # Guardian Loop with the Gazebo cabin, guided (see gazebo-sim/README.md)
./gazebo-sim/run.sh demo --hvac --fault       # ... plus ROS 2 HVAC that breaks down, so the window opens
```

The full walkthrough is in [docs/Tutorial.md](docs/Tutorial.md).

---

## Documentation

| Document | Read it when |
| --- | --- |
| [docs/Achieved.md](docs/Achieved.md) | You want the honest recap: what we built, what we did not, and why |
| [docs/Tutorial.md](docs/Tutorial.md) | You want the stack running and explained service by service |
| [notification/README.md](notification/README.md) | You want the EWS WebSocket API or Telegram notifications |
| [docs/Guardian-loop.md](docs/Guardian-loop.md) | You are building a component and need to know which existing project to copy from |
| [deploy/AUTOSD_ON_PI.md](deploy/AUTOSD_ON_PI.md) | You want the Guardian running on AutoSD, and the three blockers we hit |
| [firmware/S32K148_HARDWARE_BRINGUP.md](firmware/S32K148_HARDWARE_BRINGUP.md) | You are bringing up the S32K148 and the automotive Ethernet link |
| [docs/structure.drawio](docs/structure.drawio) | You need the editable architecture diagram |

---

## Definition of Done

**Core challenge**

- [x] Sensor data reaches the Guardian
- [x] Communication exclusively via service interfaces
- [x] Guardian evaluates risk and exposes its state on `:8080/state`
- [x] Guardian can trigger a mitigation action
- [x] Simulated ECU and actuator integration
- [x] uProtocol pub/sub implemented
- [x] uProtocol RPC implemented
- [x] Guardian operates transport-independently
- [x] An endpoint is replaced without touching the business logic, and we demonstrate it

**Full challenge**

- [x] Guardian running on AutoSD (Guardian and zenohd only, see Stage 3)
- [ ] openDuT manages the topology change
- [x] At least one physical embedded endpoint (AZ3166 with ThreadX, plus the S32K148 with OpenBSW)
- [x] Identical service artifacts before and after the configuration change

---

## AI Usage

We follow the Eclipse Foundation's
[Generative Artificial Intelligence Usage Guidelines for Eclipse Committers](https://www.eclipse.org/projects/guidelines/genai)
(version 1.0, April 2024). In short, the guidelines ask us to be transparent
about the generative AI platforms we used, to verify the accuracy of
generated output through our normal vetting (testing, intellectual property
due diligence, security), and to respect intellectual property and the
platform's terms of use. They suggest disclosing AI use in a comment just
below the copyright and licence header.

**Tools used**

| Tool | Used for |
|---|---|
| Claude Code with Anthropic Claude Opus 5.5 (`claude-opus-5-5`) | `gazebo-sim/` (Gazebo simulation backend, `run.sh`, ROS side of the bridge) and `services/src/bin/gazebo_bridge.rs` (uProtocol side of the bridge): code, configuration, tests and their documentation, plus the related additions to `docker-compose.yml` (the `gazebo-sim` and `gazebo-bridge` services), to `Containerfile`, to this README and [NOTICE.md](NOTICE.md) |
| Claude Code with Anthropic Claude Fable 5.1 (`claude-fable-5-1`) | the separate Gazebo server/GUI start in `gazebo-sim/launch/gazebo_sim.launch.py`, and the publish-race fix in `services/src/bin/window_controller_sim.rs` (marked with `Assisted-by` comments in that file) |
| Claude Code with Anthropic Claude Sonnet 5 | AZ3166 serial bridge, `deploy/setup-raspi-guardian-node.sh`, the AutoSD and S32K148 bring-up documents |
| Claude Code with Anthropic Claude Opus 5 | project overview, understanding the tasks, summarising the READMEs, [docs/Achieved.md](docs/Achieved.md) |
| Claude Code with Anthropic Claude Opus 5.5 (`claude-opus-5-5`) | reviewing and restructuring this README and the documentation |

**How we mark it.** These are our project conventions on top of the
guidelines, not requirements of the guidelines themselves:

- Files that are largely AI-generated carry a header with the Apache-2.0
  copyright notice, an *AI Disclosure* paragraph stating that the
  AI-generated portions are made available under CC0-1.0, the SPDX
  identifier `Apache-2.0 AND CC0-1.0`, and an `Assisted-by:` line naming the
  model. See also [NOTICE.md](NOTICE.md).
- Commits that used AI assistance carry an `Assisted-by:` trailer, e.g.
  `Assisted-by: Anthropic Claude Opus 5.5 (claude-opus-5-5)`.
- Files we edited only partly with AI assistance (for example this README or
  `docker-compose.yml`, which come from the reference stack or from team
  members) keep their existing headers; the AI-assisted changes are
  identifiable through the commit trailers.

**Review.** All contributions, AI-assisted or not, are reviewed by a human
team member before they are merged, and AI-generated code is tested like any
other code.

**Older commits.** Commits that were pushed before we adopted these
conventions may have been created with AI assistance but do not carry an
`Assisted-by:` trailer. We do not rewrite published history to add it.

**AI-generated files without a header.** Files without a comment syntax
(for example JSON) cannot carry the header and are listed here instead:
currently none.
