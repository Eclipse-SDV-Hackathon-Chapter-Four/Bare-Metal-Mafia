# Bare-Metal-Mafia
Hack to the Future – Guardian Loop: portable child presence detection that moves from simulation to real hardware without changing the feature. Eclipse SDV Hackathon 2026.

The Guardian Loop code should stay the same with simulation and real HW

The Guardian Loop should not call any service direktly:
CAN, GPIO, UDS, DoIP, SOME/IP, Serial Ports, ECU DIDs, Hardware Addresses

The main Guardian Loop service should ultimately run in the provided HPC environment based on AutoSD
AutoSD HPC
├── Guardian Loop
├── uProtocol runtime / transport
├── Actuation Adapter
├── optional eCall service
└── logging / diagnostics

need more AZ3166!!!

Goals:

1. Get OpenDuT working for the Testbench
2. Sensorik switch to real HW
3. planning and adding usefull features to the guardian-Loop (e.g. rate of change, sonsor confidence, multiple sensors)
4. Suport for ROS2
5. From README original make point Development Journey (https://github.com/Eclipse-SDV-Hackathon-Chapter-Four/Hack-to-the-Future#development-journey) with demo

usage:
- uProtocol communication between application-level services
- RPC when one service asks another service to perform an operation
- 


Building Blocks
- Guaridna Loop
- Child Presence Sensor
- Temeratur Sensor (already done) (The important part is the end-to-end SDV architecture?!!!)
- AZ3166 + Eclipse ThreadX Temperature Sensor
