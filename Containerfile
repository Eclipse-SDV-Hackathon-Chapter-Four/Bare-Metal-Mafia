# syntax=docker/dockerfile:1.7
FROM docker.io/library/rust:1-bookworm AS build
ARG BIN_NAME
WORKDIR /workspace

COPY Cargo.toml ./
COPY services/Cargo.toml services/Cargo.toml

RUN mkdir -p services/src/bin \
    && printf "pub fn placeholder() {}\n" > services/src/lib.rs \
    && printf "fn main() {}\n" > services/src/bin/guardian.rs \
    && printf "fn main() {}\n" > services/src/bin/child_presence_sim.rs \
    && printf "fn main() {}\n" > services/src/bin/temperature_sim.rs \
    && printf "fn main() {}\n" > services/src/bin/actuation_adapter.rs \
    && printf "fn main() {}\n" > services/src/bin/cda_sim.rs \
    && printf "fn main() {}\n" > services/src/bin/window_controller_sim.rs \
    && printf "fn main() {}\n" > services/src/bin/someip_uprot_bridge.rs \
    && printf "fn main() {}\n" > services/src/bin/someip_window_bridge.rs

RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/workspace/target,sharing=locked \
    cargo build --target-dir /workspace/target --bin ${BIN_NAME}

COPY services/src services/src

RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/workspace/target,sharing=locked \
    mkdir -p /workspace/target \
    && printf 'Signature: 8a477f597d28d172789f06886806bc55\n' > /workspace/target/CACHEDIR.TAG \
    && cargo clean -p guardian-sil --target-dir /workspace/target \
    && cargo build --target-dir /workspace/target --bin ${BIN_NAME} \
    && mkdir -p /workspace/out \
    && cp /workspace/target/debug/${BIN_NAME} /workspace/out/service

FROM docker.io/library/debian:bookworm-slim
ARG BIN_NAME
WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build /workspace/out/service /app/service
RUN chmod +x /app/service

ENV RUST_LOG=info
ENTRYPOINT ["/app/service"]
