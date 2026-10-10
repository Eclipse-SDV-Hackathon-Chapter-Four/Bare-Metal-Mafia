# Track of the goals

We had a lot of plans and not enough time, what a shame - but the part that counts works:
the Guardian Loop never changed while the hardware underneath it did.

## The main result

`guardian.rs` and `evaluate_state` in `services/src/lib.rs` were never touched while we
swapped the world below them:

- temperature: `temperature_sim` > ThreadX in Renode > AZ3166 on real hardware
- window: `window_controller_sim` > S32K148 with OpenBSW over DoIP/UDS
- window and child seat: simulators > Gazebo cabin (`gazebo-sim/run.sh` in *replace* mode)

That is the Golden Rule of the challenge, and it holds. Everything below is detail.

## What we achieved

Summary: temperature sensor, Guardian Loop and window motor all run on real hardware.

### Hardware

- Raspberry Pi with Raspberry Pi OS > VM > AutoSD > Podman > Guardian Loop.
  Three real blockers on the way: the VM disk was too small for a Rust build,
  AutoSD boots with IPv6 disabled (zenoh had to be forced into client mode),
  and Podman's default network has no container-name DNS.
  See [deploy/AUTOSD_ON_PI.md](../deploy/AUTOSD_ON_PI.md).
- Second deployment path: `deploy/setup-raspi-guardian-node.sh` turns the Pi into a
  shared always-on node that runs the full stack with the S32K148 (automotive Ethernet)
  and the AZ3166 (USB serial) plugged straight into it. No laptop has to stay open.
- S32K148 as window motor, running OpenBSW. The Automotive Ethernet shield (TJA1101)
  reaches the Pi through the Technica media converter.
  Master/slave roles had to be found by hand: jumper off on the TJA1101 board = master,
  DIP 1 OFF on the converter = slave.
- Real diagnostic path, not a stub: `services/src/bin/s32k148_doip_bridge.rs` sends a real
  UDS `WriteDataByIdentifier` (0x2E) over DoIP to the board and reads the value back.
  We added the `WindowPosition` DID `0xCF20` to the firmware
  (`firmware/0001-windowposition-did-0xCF20.patch`).
  The full story is in [firmware/S32K148_HARDWARE_BRINGUP.md](../firmware/S32K148_HARDWARE_BRINGUP.md).
- AZ3166 running Eclipse ThreadX, serial communication to the Pi > VM > AutoSD,
  bridged to uProtocol and from there into the Guardian Loop. The value is the
  LSM6DSL die temperature, plus HTS221 humidity, both also shown on the OLED.
- The same sensor path also runs emulated in Renode (`threadx-temp-sensor/`) and over
  SOME/IP (`someip_uprot_bridge.rs`, `someip_window_bridge.rs`), so it kept moving
  while boards were unavailable.

### Guardian Loop

- Changed the Guardian Loop logic, without adding a single transport or hardware
  dependency to it.
- Lowered the thresholds to 25.0 C = WARNING and 28.5 C = CRITICAL (from 32/40),
  so Bodytemparature is enough to drive the demo.
- Added `sensor_id` to `CabinTemperatureEvent` and a second temperature sensor.
  If two sensors disagree by more than 5 C, both are marked broken.
  If every known sensor is broken, the Guardian starts cooling anyway instead of
  going quiet - a missing sensor is not the same as a safe cabin.
- Added the humidity sensor. The value is read from the HTS221, published over
  uProtocol and shown on the dashboard, so the loop can use it without any
  firmware change. `evaluate_state` does not read it yet (see below).
- Reset functionality, and an EWS warning input that can push the loop to WARNING
  from outside.
- EWS API: WebSocket on `ws://localhost:8765/ws`, a JSON `GuardianState` every second,
  `EWSWarn` messages (`Reset`, `Heat`) back in on the same connection.
- Notification service: dependency-free Java service in `notification/`, connects to
  the EWS WebSocket and sends a Telegram message on every Guardian state change.
  Reconnects when the Guardian restarts, dry-run mode when no token is set.

### Simulation and web interface

- Web interface (`:8094`): manual window open and reset, child present / absent toggle
  that publishes a real uProtocol event, and HVAC fault injection. Live temperature from
  the hardware sensor, and the LED on the board as the motor output.
- HVAC commands feed back into `temperature_sim`, so cooling actually pulls the cabin
  temperature down on screen instead of only being logged.
- Gazebo cabin simulation (`gazebo-sim/`), our own work: a rear-left window joint that
  physically moves, a rear seat with a contact sensor, and the car on a parking lot.
- The Gazebo to uProtocol bridge, also our own work: `gazebo-sim/bridge/ros_zenoh_bridge.py`
  on the ROS 2 side and `services/src/bin/gazebo_bridge.rs` on the uProtocol side, with
  two modes. *mirror* - Gazebo follows the real window state. *replace* - Gazebo
  replaces the window controller and the child presence simulator entirely.
- `gazebo-sim/run.sh` is the one entry point: `./gazebo-sim/run.sh demo` runs the whole
  guided demo (Guardian + Gazebo + HVAC) with live status lines, and
  `./gazebo-sim/run.sh test` runs the end-to-end checks.
- Documentation: [docs/Tutorial.md](Tutorial.md) walks through the stack service by
  service, from a plain `docker compose up` to the ThreadX and ROS 2 profiles.

### Cabin anomaly detection and temperature prediction

- Developed a Python-based analysis pipeline (`cabin_anomaly_detection.py`) to process
  cabin event data collected in CSV format. The pipeline validates the input schema,
  processes timestamps, reconstructs the latest known system state from event-based
  records, and performs feature engineering.

- Implemented explainable, rule-based anomaly detection for three categories:

  - **HVAC anomalies:** Detects HVAC stuck OFF when a child is present and the
    temperature exceeds the target by more than 2°C, ineffective HVAC behavior when
    the temperature continues rising despite HVAC operation, and explicitly reported
    HVAC faults.

  - **Temperature anomalies:** Identifies temperature rises or drops of at least 2°C
    based on future temperature observations within the following 10 minutes, and
    flags degraded temperature sensor status.

  - **Child-safety anomalies:** Assigns warning, high and critical temperature levels
    at 26°C, 28°C and 30°C respectively when a child is present. It also identifies
    the combination of a present child, HVAC OFF and temperature of at least 26°C
    as an additional risk condition.

- Combined individual rule outputs into `HVAC_ANOMALY`, `TEMPERATURE_ANOMALY` and
  `CHILD_SAFETY_ANOMALY` indicators, with a separate child-safety severity
  classification.

- Developed a separate machine-learning pipeline to predict how cabin temperature
  is likely to behave over the next 10 minutes when a child is present. It uses
  16 current-state and engineered features covering temperature, child confidence,
  HVAC status and settings, window information, and temperature trends.

- Compared Logistic Regression as a baseline classifier against XGBoost for three
  prediction classes: `NORMAL`, `SPIKE` and `DROP`. Applied class weighting in
  XGBoost to address class imbalance and evaluated both models using accuracy,
  balanced accuracy, macro F1, class-wise metrics and confusion matrices.

- Generated analysis outputs under `results/`, including
  `anomaly_detection_results.csv`, `temperature_prediction_dataset.csv` and
  `model_results.csv`, along with visualizations of event distributions, anomaly
  categories, temperature timelines, child-safety conditions, model confusion
  matrices and feature importance.

- **Preliminary model evaluation:** On the current randomized test split, Logistic
  Regression achieved 90.53% accuracy, while XGBoost achieved 98.82% accuracy,
  98.04% balanced accuracy and 94.23% macro F1. These results demonstrate the
  potential of XGBoost on the experimental dataset; further evaluation on
  chronologically held-out data and additional real-world observations is needed
  to assess generalization.

### Found on the way

- Publish race in `services/src/bin/window_controller_sim.rs`: a stale window state could
  be published after a newer one. Found while testing the Gazebo bridge, fixed, and marked
  with `Assisted-by` comments in that file.

## What we didn't achieve

- **openDuT.** Not started. It is an explicit Definition-of-Done item
  ("openDuT manages the topology change") and the one part of the full challenge we
  never reached. The Vagrantfile brings the vcan / can-gw groundwork, nothing more.
- **Real OpenSOVD CDA in the stack.** `cda_sim` was never replaced by the real CDA.
  The setup script exists (`firmware/setup-run-sovd-cda.sh`), and `WindowPosition`
  still has to be added to the generated MDD database before the CDA knows the DID.
- **AutoSD beyond the Guardian.** Only `guardian` and `zenohd` were ever proven inside
  the VM. No systemd/Podman quadlets, and no hardware passthrough into the VM - the
  bare-Pi path was used for the hardware instead.
- **Visible physical actuator on the S32K.** `firmware/0002-window-led-blink.patch`
  (blink EVAL_LED_RED while the window is open) is written and verified against the
  source, but not built and flashed.
- **Cold warning** (someone sits too long in a cold car). Actually implemented on the
  branch `expanded-temperature-checks`: `DangerReason::{Heat, Cold}`, WARNING at
  <= 20 C, CRITICAL at <= 15 C, and a cold mitigation path separate from the heat one.
  It was not merged into `main` in time.
- **Humidity inside the decision.** Published and displayed, but `evaluate_state` does
  not use it yet.
- **Sensor confidence and rate of change.** Both were on our list as the interesting
  loop improvements, neither was implemented. The child presence event already carries
  a confidence of 0.98 and we still ignore it.

## Thoughts and notes

We decided to use the humidity sensor as well, because a body overheats faster in high
humidity. It can also be used for a mould warning.

Ideally we would use three temperature sensors: one for the outside temperature and two
redundant ones for the passenger compartment. The disagreement logic we built already
expects more than one.

## Usage of AI

We follow the Eclipse Foundation's
[Generative AI Usage Guidelines](https://www.eclipse.org/projects/guidelines/genai).
Which models we used for what, and how AI-assisted files and commits are marked, is
listed in one place: the [AI Usage section of the README](../README.md#ai-usage),
together with [NOTICE.md](../NOTICE.md).
