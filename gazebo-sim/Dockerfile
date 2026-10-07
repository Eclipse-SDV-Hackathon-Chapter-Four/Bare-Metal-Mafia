# syntax=docker/dockerfile:1.7
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

#
# Image for the gazebo-sim compose service (profile: gazebo). Developer
# laptops only; not part of any Raspberry Pi / HPC deployment.
#
#   ROS 2 Humble + Gazebo Fortress (ign-gazebo 6), the Gazebo release paired
#   with Humble. ros-humble-ros-gz-* pull Fortress (libignition-gazebo6,
#   ignition-msgs8, ignition-transport11) as dependencies; no Harmonic, no
#   Gazebo Classic.
#
# Contents: the cabin world, the bridge and window configs, the launch file
# that starts Gazebo and ros_gz_bridge together, and tools/sim_check.py used
# by test-window.sh. See gazebo-sim/README.md.
FROM docker.io/library/ros:humble-ros-base
ARG ROS_DISTRO=humble

# Only the pieces we use (not the ros-humble-ros-gz meta-package, which adds
# ros_gz_image and the demos):
#   ros-gz-sim         Gazebo Fortress itself plus the `ign` CLI
#   ros-gz-bridge      parameter_bridge with YAML config_file support
#   ros-gz-interfaces  ros_gz_interfaces/msg/Contacts
#   libqt5svg5         SVG icons in the optional GUI (a recommends-only
#                      dependency, skipped by --no-install-recommends)
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        libqt5svg5 \
        ros-${ROS_DISTRO}-ros-gz-sim \
        ros-${ROS_DISTRO}-ros-gz-bridge \
        ros-${ROS_DISTRO}-ros-gz-interfaces \
    && rm -rf /var/lib/apt/lists/*

COPY gazebo-sim/worlds /opt/gazebo_sim/worlds
COPY gazebo-sim/config /opt/gazebo_sim/config
COPY gazebo-sim/launch /opt/gazebo_sim/launch
COPY gazebo-sim/tools /opt/gazebo_sim/tools
RUN chmod +x /opt/gazebo_sim/tools/sim_check.py

# ROS_DOMAIN_ID 42 keeps this ROS graph apart from ros2-hvac (domain 0);
# IGN_PARTITION does the same for Gazebo Transport. Both are repeated in
# docker-compose.yml so they are visible where the stack is configured.
ENV RMW_IMPLEMENTATION=rmw_fastrtps_cpp \
    ROS_DOMAIN_ID=42 \
    IGN_PARTITION=guardian_sim \
    GAZEBO_SIM_SHARE=/opt/gazebo_sim \
    GZ_GUI=false \
    ROS_DISTRO=humble

# /ros_entrypoint.sh (from the base image) sources /opt/ros/humble.
CMD ["ros2", "launch", "/opt/gazebo_sim/launch/gazebo_sim.launch.py"]
