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

# Image for ros_up_mapper, the uProtocol side of ros-up-bridge. Its own
# Dockerfile (instead of the shared Containerfile) so the shared one stays
# untouched, and built with --locked so zenoh stays at the Cargo.lock version
# (1.9.0), matching eclipse-zenoh==1.9.0 on the ROS side.
FROM docker.io/library/rust:1-bookworm AS build
WORKDIR /workspace

COPY Cargo.toml Cargo.lock ./
COPY services/Cargo.toml services/Cargo.toml
COPY services/src services/src

RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/workspace/target-ros-up,sharing=locked \
    cargo build --locked --target-dir /workspace/target-ros-up --bin ros_up_mapper \
    && mkdir -p /out \
    && cp /workspace/target-ros-up/debug/ros_up_mapper /out/ros_up_mapper

FROM docker.io/library/debian:bookworm-slim
WORKDIR /app

COPY --from=build /out/ros_up_mapper /app/ros_up_mapper
# Mapping files, and window.yaml at the same path as in the gazebo-sim image
# so ${window.*} constants resolve identically on both sides.
COPY ros-up-bridge/config /opt/ros_up_bridge/config
COPY gazebo-sim/config/window.yaml /opt/gazebo_sim/config/window.yaml

ENV RUST_LOG=ros_up_mapper=info,info
ENTRYPOINT ["/app/ros_up_mapper"]
CMD ["--config", "/opt/ros_up_bridge/config/mirror.yaml"]
