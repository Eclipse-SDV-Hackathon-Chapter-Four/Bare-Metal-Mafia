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

//! Route runtime: wires each route's inputs (uProtocol listeners or Zenoh
//! subscribers) to a per-route task that processes messages in order and
//! sends the result to the route's output.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use guardian_sil::{decode_json_payload, publish_json_event};
use serde_json::{json, Value};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tracing::{debug, info, warn};
use up_rust::{UListener, UMessage, UTransport, UUri};

use crate::config::{resolve_uri, ConfigError, EndpointCfg, MappingFile, RouteCfg};
use crate::transform::{map_fields, Context};

#[derive(Default)]
pub struct RouteStats {
    pub received: AtomicU64,
    pub sent: AtomicU64,
    pub errors: AtomicU64,
    /// Last input message, for diagnosis (e.g. what was last on the bus).
    pub last_input: std::sync::Mutex<Option<Value>>,
}

#[derive(Default)]
pub struct Stats {
    pub uprotocol_published: AtomicU64,
    pub zenoh_published: AtomicU64,
    pub routes: BTreeMap<String, Arc<RouteStats>>,
}

impl Stats {
    pub fn to_json(&self) -> Value {
        let routes: serde_json::Map<String, Value> = self
            .routes
            .iter()
            .map(|(name, s)| {
                (
                    name.clone(),
                    json!({
                        "received": s.received.load(Ordering::Relaxed),
                        "sent": s.sent.load(Ordering::Relaxed),
                        "errors": s.errors.load(Ordering::Relaxed),
                        "last_input": s.last_input.lock().map(|v| v.clone()).unwrap_or(None),
                    }),
                )
            })
            .collect();
        json!({
            "uprotocol_published": self.uprotocol_published.load(Ordering::Relaxed),
            "zenoh_published": self.zenoh_published.load(Ordering::Relaxed),
            "routes": routes,
        })
    }
}

#[derive(Clone, Debug)]
pub enum Endpoint {
    Up(UUri),
    /// Full Zenoh key of a link.
    Link(String),
}

impl Endpoint {
    fn resolve(cfg: &EndpointCfg, key_prefix: &str) -> Result<Self, ConfigError> {
        Ok(match cfg {
            EndpointCfg::Uprotocol(u) => Endpoint::Up(resolve_uri(u)?),
            EndpointCfg::Link(name) => Endpoint::Link(format!("{key_prefix}/{name}")),
        })
    }
}

pub struct RouteInput {
    /// Index into the route's inputs (used by routes with several inputs).
    #[allow(dead_code)]
    pub input: usize,
    pub value: Value,
}

/// Shared transports and counters.
pub struct Io {
    pub transport: Arc<dyn UTransport>,
    pub zenoh: zenoh::Session,
    pub stats: Arc<Stats>,
}

impl Io {
    pub async fn send(&self, endpoint: &Endpoint, value: &Value) -> Result<(), String> {
        match endpoint {
            Endpoint::Up(uri) => {
                publish_json_event(self.transport.clone(), uri.clone(), value)
                    .await
                    .map_err(|e| format!("uProtocol publish failed: {e}"))?;
                self.stats.uprotocol_published.fetch_add(1, Ordering::Relaxed);
            }
            Endpoint::Link(key) => {
                let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
                self.zenoh
                    .put(key.as_str(), bytes)
                    .await
                    .map_err(|e| format!("zenoh put {key} failed: {e}"))?;
                self.stats.zenoh_published.fetch_add(1, Ordering::Relaxed);
            }
        }
        Ok(())
    }

    async fn subscribe(
        &self,
        endpoint: &Endpoint,
        tx: UnboundedSender<RouteInput>,
        input: usize,
    ) -> Result<(), ConfigError> {
        match endpoint {
            Endpoint::Up(uri) => {
                self.transport
                    .register_listener(uri, None, Arc::new(UpInput { tx, input }))
                    .await
                    .map_err(|e| format!("register_listener failed: {e}"))?;
            }
            Endpoint::Link(key) => {
                let key_for_log = key.clone();
                self.zenoh
                    .declare_subscriber(key.as_str())
                    .callback(move |sample| {
                        match serde_json::from_slice::<Value>(&sample.payload().to_bytes()) {
                            Ok(value) => {
                                let _ = tx.send(RouteInput { input, value });
                            }
                            Err(err) => warn!("invalid JSON on {key_for_log}: {err}"),
                        }
                    })
                    .background()
                    .await?;
            }
        }
        Ok(())
    }
}

struct UpInput {
    tx: UnboundedSender<RouteInput>,
    input: usize,
}

#[async_trait]
impl UListener for UpInput {
    async fn on_receive(&self, message: UMessage) {
        match decode_json_payload::<Value>(&message) {
            Ok(value) => {
                let _ = self.tx.send(RouteInput { input: self.input, value });
            }
            Err(err) => warn!("invalid uProtocol JSON payload: {err}"),
        }
    }
}

/// Subscribe every route's inputs and spawn its task. Returns the counters.
pub async fn start(
    cfg: &MappingFile,
    transport: Arc<dyn UTransport>,
    zenoh: zenoh::Session,
) -> Result<(Arc<Io>, Arc<Stats>), ConfigError> {
    let mut stats = Stats::default();
    for route in &cfg.routes {
        stats.routes.insert(route.name().to_string(), Arc::new(RouteStats::default()));
    }
    let stats = Arc::new(stats);
    let io = Arc::new(Io { transport, zenoh, stats: stats.clone() });
    let prefix = cfg.zenoh.key_prefix.as_str();

    for route in &cfg.routes {
        let route_stats = stats.routes[route.name()].clone();
        let (tx, rx) = unbounded_channel();
        for (i, input) in route.inputs().into_iter().enumerate() {
            io.subscribe(&Endpoint::resolve(input, prefix)?, tx.clone(), i).await?;
        }
        match route {
            RouteCfg::Forward { name, to, fields, repeat_last_ms, .. } => {
                let to = Endpoint::resolve(to, prefix)?;
                info!(
                    "route {name} (forward): {:?} -> {:?}, repeat_last_ms {:?}",
                    route.inputs()[0],
                    to,
                    repeat_last_ms
                );
                let repeat = repeat_last_ms.map(Duration::from_millis);
                tokio::spawn(run_forward(
                    name.clone(),
                    rx,
                    to,
                    fields.clone(),
                    repeat,
                    io.clone(),
                    route_stats,
                ));
            }
        }
    }
    Ok((io, stats))
}

async fn run_forward(
    name: String,
    mut rx: UnboundedReceiver<RouteInput>,
    to: Endpoint,
    fields: BTreeMap<String, crate::transform::FieldSpec>,
    repeat: Option<Duration>,
    io: Arc<Io>,
    stats: Arc<RouteStats>,
) {
    let mut last: Option<Value> = None;
    let mut ticker = tokio::time::interval(repeat.unwrap_or(Duration::from_secs(3600)));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        let msg = tokio::select! {
            msg = rx.recv() => match msg {
                Some(msg) => msg,
                None => return,
            },
            _ = ticker.tick(), if repeat.is_some() => {
                if let Some(out) = &last {
                    if let Err(err) = io.send(&to, out).await {
                        warn!("route {name}: repeat failed: {err}");
                    }
                }
                continue;
            }
        };
        stats.received.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut last_input) = stats.last_input.lock() {
            *last_input = Some(msg.value.clone());
        }
        let ctx = Context { input: Some(&msg.value), ..Default::default() };
        let result = match map_fields(&fields, &ctx) {
            Ok(out) => io.send(&to, &out).await.map(|_| out),
            Err(e) => Err(e),
        };
        match result {
            Ok(out) => {
                stats.sent.fetch_add(1, Ordering::Relaxed);
                debug!("route {name}: {} -> {}", msg.value, out);
                last = Some(out);
            }
            Err(err) => {
                stats.errors.fetch_add(1, Ordering::Relaxed);
                warn!("route {name}: dropped message: {err}");
            }
        }
    }
}
