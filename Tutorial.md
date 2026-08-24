# Tutorial
# hack-to-the-future

Minimal Rust-based demo for a child-in-car guardian flow using uProtocol over Zenoh.

## What is in this repo

- `guardian`: central decision service.
- `child_presence_sim`: publishes child presence events.
- `temperature_sim`: software temperature simulator for the closed loop. It exists, but is currently commented out in [`docker-compose.yml`]
- `actuation_adapter`: receives mitigation requests from `guardian`.
- `cda_sim`: simulated diagnostics layer.
- `window_controller_sim`: simulated window actuator.
- `someip_uprot_bridge`: converts SOME/IP temperature messages to uProtocol.
- `someip_window_bridge`: converts window-state uProtocol events back to SOME/IP.
- `threadx-temp-sensor/`: Eclipse ThreadX temperature sensor used for the ThreadX/SOME-IP path.

## Prerequisites

- Rust and Cargo
- Docker with Docker Compose

## Quick start

Check that the workspace builds:

```bash
cargo check --workspace
```

Start the default stack:

```bash
docker compose up --build
```

This starts:

- `zenohd`
- `guardian`
- `child-presence-sim`
- `actuation-adapter`
- `cda-sim`
- `window-controller-sim`

Exposed ports:

- `7447`: `zenohd`
- `8080`: `guardian`
- `8092`: `window-controller-sim`

## ThreadX / SOME-IP path

The ThreadX-based temperature sensor and BOTH SOME/IP bridges are available through the `threadx` compose profile:

```bash
docker compose --profile threadx up --build
```

That adds:

- `threadx-temp-sensor`
- `someip-uprot-bridge`
- `someip-window-bridge`

Notes:

- `temperature_sim` is disabled in the current compose file.
- `someip-uprot-bridge` exposes UDP `30501`.
- The embedded sensor sources live in [`threadx-temp-sensor/`]

## Current Flow Diagram

```mermaid
flowchart LR
	CPS[child_presence_sim] -->|publish
up/sdv/guardian/vss/Vehicle.Cabin.Seat.Row2.ChildPresence| Z[zenohd]
	TS[temperature_sim] -->|publish
up/sdv/guardian/vss/Vehicle.Cabin.HVAC.AmbientAirTemperature| Z

	TX[threadx-temp-sensor
ThreadX Linux port
or Renode STM32F407] -->|SOME/IP UDP
Service 0x1234 / Event 0x8001| BR[someip-uprot-bridge]
	BR -->|publish
up/sdv/guardian/vss/Vehicle.Cabin.HVAC.AmbientAirTemperature| Z

	Z -->|receive VSS events via UTransport| G[guardian]
	G <-->|RPC invoke mitigation method| Z

	Z -->|RPC endpoint registered| AA[actuation_adapter]
	AA -->|publish diag window/alarm cmd
up/sdv/guardian/diag/*| Z

	Z -->|subscribe diag cmd| CDA[cda_sim]
	CDA -->|publish uds window/alarm cmd
up/sdv/guardian/uds/*| Z

	Z -->|subscribe uds cmd| WC[window_controller_sim]
	WC -->|publish window state
up/sdv/guardian/vss/Vehicle.Cabin.Window.Row2.Left.State| Z

	Z -->|receive window state| TS
```

## Current Architecture Diagram

```mermaid
flowchart TB
	subgraph Sensors[Sensor Services]
		CPS[child_presence_sim]
		TS[temperature_sim\nStage A/B]
		TX[threadx-temp-sensor\nEclipse ThreadX\nStage C]
	end

	subgraph SomeipBridge[SOME/IP Bridge]
		BR[someip-uprot-bridge\nSOME/IP → uProtocol]
	end

	subgraph Decision[Decision Service]
		G[guardian]
	end

	subgraph Actuation[Actuation and Diagnostics]
		AA[actuation_adapter]
		CDA[cda_sim]
		WC[window_controller_sim]
	end

	subgraph Transport[Transport Layer]
		Z[zenohd]
	end

	CPS <--> Z
	TS <--> Z
	TX -- SOME/IP UDP --> BR
	BR <--> Z
	G <--> Z
	AA <--> Z
	CDA <--> Z
	WC <--> Z

	TS -.closed-loop thermal feedback via VSS window state.-> Z
```
