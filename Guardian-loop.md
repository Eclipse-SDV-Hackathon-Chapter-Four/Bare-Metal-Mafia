# Guardian Loop — SDV Building Blocks

The **Guardian Loop** should not be implemented from scratch.

A large part of the functionality needed for this challenge already exists as examples in the Eclipse SDV ecosystem.

The goal is to **reuse existing SDV patterns as building blocks** and combine them into one end-to-end feature:

> **Sense → Communicate → Decide → Actuate → Replace Simulation with Hardware**

The main challenge is integration and portability.

The Guardian business logic should remain unchanged when simulated endpoints are replaced with physical hardware.

---

# 🧱 Building Block Overview

| What you need | Example / Building Block | What to reuse |
|---|---|---|
| uProtocol RPC Client | [Service-to-Signal Horn Client](https://github.com/eclipse-sdv-blueprints/service-to-signal/blob/main/components/horn-client/src/horn_client.rs) | Rust pattern for invoking a uProtocol service |
| uProtocol Service | [Horn Service Kuksa](https://github.com/eclipse-sdv-blueprints/service-to-signal/tree/main/components/horn-service-kuksa) | Example uProtocol service provider |
| Service → Signal → Hardware | [Service-to-Signal Blueprint](https://github.com/eclipse-sdv-blueprints/service-to-signal) | Complete service-to-embedded-actuator flow |
| Embedded Actuator | [Actuator Provider](https://github.com/eclipse-sdv-blueprints/service-to-signal/tree/main/components/actuator-provider) | ESP32 / Zenoh hardware integration |
| Transport-independent uProtocol | [Fleet Management](https://github.com/eclipse-sdv-blueprints/fleet-management) | Same application logic over different transports |
| Vehicle Application + Signals | [Companion Application](https://github.com/eclipse-sdv-blueprints/companion-application) | Vehicle application reading and actuating vehicle signals |
| uService → SOVD → UDS | [Commercial SDV Stack](https://github.com/eclipse-sdv-blueprints/commercial-sdv-stack) | High-level service controlling a classic ECU |
| uService Definition | [Powertrain AsyncAPI](https://github.com/eclipse-sdv-blueprints/commercial-sdv-stack/blob/main/uservices/powertrain/Powertrain-asyncapi.yaml) | Template for defining Guardian / Window services |
| SOVD + OpenBSW | [OpenBSW SOVD Demo](https://github.com/Eclipse-SDV-HackFest-Esslingen-2026/OpenBSW-Playground/tree/main/OpenBSW-SOVD-Demo) | SOVD → DoIP → UDS → OpenBSW |
| ThreadX Sensor ECU | [AZ3166 ThreadX Example](https://github.com/chheis/challenge-threadx-playRemote/blob/4f9cac54efcd383f1cadcedb4aa3c93a97ba9dd0/MXChip/AZ3166/app/main.c#L770) | Temperature sensor running on embedded hardware |
| HPC + MCUs + CAN | [E2E Vehicle Signals](https://github.com/eclipse-sdv-blueprints/e2e-vehicle-signals) | Physical HPC / MCU / CAN / ThreadX integration |
| Software Orchestration | [Software Orchestration Blueprint](https://github.com/eclipse-sdv-blueprints/software-orchestration) | Ankaios / BlueChi workload deployment |
| Dynamic Edge Software | [ROS Racer](https://github.com/eclipse-sdv-blueprints/ros-racer) | Dynamic deployment, OTA and rollback patterns |
| Hazard / Event Processing | [Insurance Blueprint](https://github.com/eclipse-sdv-blueprints/insurance) | Signal monitoring and event detection pattern |

---

# 🧩 How the Pieces Fit Together

The Guardian Loop itself should stay relatively small.

```text
                     ┌─────────────────────┐
                     │    Guardian Loop    │
                     │   Rust / AutoSD     │
                     └──────────┬──────────┘
                                │
             ┌──────────────────┼──────────────────┐
             │                  │                  │
             ▼                  ▼                  ▼

      Child Presence      Cabin Temperature     Vehicle Action
          Service              Service              Service

             │                  │                  │
             │                  │                  ▼
             │                  │             SOVD / CDA
             │                  │                  │
             │                  │                  ▼
             │                  │              DoIP / UDS
             │                  │                  │
             │                  │                  ▼
             │                  │               OpenBSW
             │                  │                  │
             │                  │                  ▼
             │                  │            Window / Fan / Horn
