// Copyright (c) 2026 Contributors to the Bare-Metal-Mafia project
// See the NOTICE file(s) distributed with this work for additional
// information regarding copyright ownership.
//
// This program and the accompanying materials are made available under the
// terms of the Apache License, Version 2.0 which is available at
// https://www.apache.org/licenses/LICENSE-2.0
//
// AI Disclosure: This file was largely AI-generated. The AI-generated
// portions are made available under CC0-1.0 and not subject to the
// project's licence. The human contributor has reviewed and verified
// that the code is correct.
//
// SPDX-License-Identifier: Apache-2.0 AND CC0-1.0
// Assisted-by: Anthropic Claude Opus 5.5 (claude-opus-5-5)
// Assisted-by: Anthropic Claude Fable 5.1 (claude-fable-5-1)

//! `ros_up_mapper`: the uProtocol side of ros-up-bridge.
//!
//! The ROS side (ros-up-bridge/ros_side/ros_zenoh_bridge.py, in the
//! gazebo-sim container) turns ROS 2 messages into JSON on plain Zenoh keys.
//! This binary maps those keys to and from uProtocol URIs, using the same
//! mapping YAML, so uProtocol is spoken only by the Rust up-rust /
//! up-transport-zenoh stack the rest of the services use.
//!
//!   ros_up_mapper --config /opt/ros_up_bridge/config/mirror.yaml
//!
//! Environment: ZENOH_CONNECT (Zenoh router endpoint), RUST_LOG,
//! ROS_UP_MAPPER_CONFIG (instead of --config), ROS_UP_STRICT_SINGLE_PUBLISHER
//! (1 = exit with code 1 when a second publisher is detected, see guard.rs).
//!
//! HTTP (port from the mapping's `http.port`): GET /health (503 once a second
//! publisher was seen), GET /stats, plus the `http.path` of state routes.

mod config;
mod engine;
mod guard;
mod routes;
mod transform;

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use guardian_sil::{make_uri_provider, open_up_transport, open_zenoh_session};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tracing::info;

use crate::config::Mode;
use crate::engine::Stats;
use crate::guard::Guard;

#[derive(Clone)]
struct HttpState {
    mode: Mode,
    stats: Arc<Stats>,
    guard: Arc<Guard>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "ros_up_mapper=info,info".to_string()),
        )
        .init();

    let config_path = config_path()?;
    let cfg = config::load(&config_path).map_err(|e| e.to_string())?;
    info!(
        "mapping {} loaded: mode {:?}, {} route(s)",
        config_path.display(),
        cfg.mode,
        cfg.routes.len()
    );

    let transport = open_up_transport(make_uri_provider("ros-up-mapper", 0x9601, 0x01)).await?;
    let zenoh = open_zenoh_session()
        .await
        .map_err(|e| format!("zenoh open failed: {e}"))?;
    let guard = Guard::from_env();
    let (io, http_states) = engine::start(&cfg, transport, zenoh, guard.clone())
        .await
        .map_err(|e| e.to_string())?;

    let Some(http) = &cfg.http else {
        tokio::signal::ctrl_c().await?;
        return Ok(());
    };
    let mut app = Router::new()
        .route("/health", get(health))
        .route("/stats", get(get_stats))
        .with_state(HttpState {
            mode: cfg.mode,
            stats: io.stats.clone(),
            guard,
        });
    for state in http_states {
        let engine::HttpState {
            path,
            fields,
            state,
        } = state;
        app = app.route(
            &path,
            get(move || {
                let snapshot: serde_json::Map<String, Value> = state
                    .lock()
                    .map(|s| {
                        fields
                            .iter()
                            .filter_map(|f| s.get(f).map(|v| (f.clone(), v.clone())))
                            .collect()
                    })
                    .unwrap_or_default();
                async move { Json(Value::Object(snapshot)) }
            }),
        );
    }
    let addr = format!("0.0.0.0:{}", http.port);
    let listener = TcpListener::bind(&addr).await?;
    info!("ros_up_mapper HTTP on {addr} (/health, /stats)");
    axum::serve(listener, app).await?;
    Ok(())
}

fn config_path() -> Result<PathBuf, String> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--config" {
            return args
                .next()
                .map(PathBuf::from)
                .ok_or_else(|| "--config needs a path".into());
        }
    }
    std::env::var("ROS_UP_MAPPER_CONFIG")
        .map(PathBuf::from)
        .map_err(|_| "usage: ros_up_mapper --config <mapping.yaml>".into())
}

async fn health(State(app): State<HttpState>) -> (StatusCode, Json<Value>) {
    let mode = format!("{:?}", app.mode).to_lowercase();
    if app.guard.healthy() {
        (StatusCode::OK, Json(json!({"status": "ok", "mode": mode})))
    } else {
        let n = app
            .guard
            .foreign_events
            .load(std::sync::atomic::Ordering::Relaxed);
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"status": "unhealthy", "mode": mode,
                        "reason": format!("second publisher detected ({n} foreign message(s))")})),
        )
    }
}

async fn get_stats(State(app): State<HttpState>) -> Json<Value> {
    let mut stats = app.stats.to_json();
    stats["mode"] = json!(format!("{:?}", app.mode).to_lowercase());
    stats["foreign_publisher_events"] = json!(app
        .guard
        .foreign_events
        .load(std::sync::atomic::Ordering::Relaxed));
    stats["strict_single_publisher"] = json!(app.guard.strict());
    Json(stats)
}
