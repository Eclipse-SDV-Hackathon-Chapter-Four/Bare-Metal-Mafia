# Track of the goals

We had a lot of plans but not enough time, what a shame

race condition found in loop.rs

## What we achieved

summary: we got the temp sensor, guardianloop and window motor on HW

- Raspberry pi with Rapberry OS > VM > AutoSD > Podman > Guardian Loop
- S32K as window motor with the Automotive Ethernet shield gets the signal over the mediaconverter from the Ras pi (loop > adapter > output)
- S32K running with OpenBSW
- AZ3166 ThreadX running with Serial communikation to the Ras pi > vm > AutoSD with a bridge to uProtocol followed by the guardian loop
- Web interface: possible reset of the Window state and child state - also visual input temparatur change from HW and also lamp as Motor output on HW
- changed boundary values for test purpos; added Humidity as value for Guardian loop

- Change the Guardian Loop
- reset functionality

- notification to telegramm message if the state changes
- API to Guarian loop
- Added new Sensors with id: we added a second temp sensor with fault marking if the values are different (and the humidity Sensor)

- uProtocoll to ROS2 for Gazebo Sim of the Car (with child, fan and windows)

## What we didnt achieve

- cold warning (someone is to long in a cold car)

### Thoughts and Nodes

We decides that we use also the humidity sensor as Information because a body can overheat faster with high Humidity.
Aditional it can be used to give a mold warning.
Also the ideal would be, to use 3 Temp sensors - one for the outside Temp and two redundant for the passenger room.

## Usage of AI

Opus 5 was used for an overview of the Project, unterstanding the tasks and summering the Readmes.
