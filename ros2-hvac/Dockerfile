# syntax=docker/dockerfile:1.7

FROM docker.io/library/rust:1-bookworm AS rust-build
WORKDIR /workspace

COPY Cargo.toml Cargo.lock ./
COPY services/Cargo.toml services/Cargo.toml
COPY services/src services/src

RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/workspace/target,sharing=locked \
    cargo build --target-dir /workspace/target --bin ros2_hvac_bridge \
    && mkdir -p /out \
    && cp /workspace/target/debug/ros2_hvac_bridge /out/ros2_hvac_bridge

FROM docker.io/library/ros:humble-ros-base
ARG ROS_DISTRO=humble

SHELL ["/bin/bash", "-lc"]

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        git \
        python3-colcon-common-extensions \
        python3-jsonschema \
        python3-pip \
        python3-rosdep \
        python3-requests \
        python3-setuptools \
        python3-vcstool \
        ros-${ROS_DISTRO}-diagnostic-msgs \
        ros-${ROS_DISTRO}-rqt \
        ros-${ROS_DISTRO}-rqt-common-plugins \
        ros-${ROS_DISTRO}-rqt-graph \
        ros-${ROS_DISTRO}-ros2-medkit-gateway \
    && rm -rf /var/lib/apt/lists/*

RUN rosdep init 2>/dev/null || true \
    && rosdep update

WORKDIR /opt/muto_ws/src
RUN git clone --depth 1 https://github.com/eclipse-muto/agent.git \
    && git clone --depth 1 https://github.com/eclipse-muto/core.git \
    && git clone --depth 1 https://github.com/eclipse-muto/messages.git \
    && git clone --depth 1 https://github.com/eclipse-muto/composer.git

WORKDIR /opt/muto_ws
RUN apt-get update \
    && source /opt/ros/${ROS_DISTRO}/setup.bash \
    && rosdep install --from-paths src --ignore-src -r -y --skip-keys="ament_python" \
    && colcon build --merge-install --cmake-args -DCMAKE_BUILD_TYPE=Release

COPY ros2-hvac/start-hvac-stack.sh /usr/local/bin/start-hvac-stack.sh
COPY ros2-hvac/runtime /opt/muto_runtime
COPY --from=rust-build /out/ros2_hvac_bridge /usr/local/bin/ros2_hvac_bridge

RUN chmod +x \
        /usr/local/bin/start-hvac-stack.sh \
        /usr/local/bin/ros2_hvac_bridge \
        /opt/muto_runtime/deploy_stack.py \
    && mkdir -p /root/.ros2_medkit

ENV MEDKIT_BASE_URL=http://127.0.0.1:8080/api/v1/ \
    MEDKIT_HVAC_APP_ID=hvac_simulator \
    MEDKIT_TARGET_TEMPERATURE_PARAM=target_temperature_celsius \
    MEDKIT_AC_ACTIVE_PARAM=air_conditioning_active \
    MEDKIT_FAN_SPEED_PARAM=fan_speed_percent \
    MEDKIT_FAULT_PARAM=fault_active \
    HVAC_BRIDGE_SYNC_INTERVAL_MS=1000 \
    RMW_IMPLEMENTATION=rmw_fastrtps_cpp \
    ROS_DOMAIN_ID=0 \
    ROS_DISTRO=humble

ENTRYPOINT ["/usr/local/bin/start-hvac-stack.sh"]
