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
// Assisted-by: Anthropic Claude Fable 5.1 (claude-fable-5-1)

//! Stateful route kinds: `state` and `presence`.

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::sync::mpsc::UnboundedReceiver;
use tracing::{debug, info, warn};

use crate::config::{StateInputCfg, StatePublishCfg};
use crate::engine::{Endpoint, Io, RouteInput, RouteStats};
use crate::transform::{map_fields, Context, FieldSpec};

pub type SharedState = Arc<Mutex<BTreeMap<String, Value>>>;

const TICK: Duration = Duration::from_millis(10);

fn record_input(stats: &RouteStats, value: &Value) {
    stats.received.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut last) = stats.last_input.lock() {
        *last = Some(value.clone());
    }
}

async fn publish(
    name: &str,
    io: &Io,
    to: &Endpoint,
    fields: &BTreeMap<String, FieldSpec>,
    state: &BTreeMap<String, Value>,
    stats: &RouteStats,
) -> bool {
    let ctx = Context {
        input: None,
        state: Some(state),
    };
    let result = match map_fields(fields, &ctx) {
        Ok(out) => io.send(to, &out).await.map(|_| out),
        Err(e) => Err(e),
    };
    match result {
        Ok(out) => {
            stats.sent.fetch_add(1, Ordering::Relaxed);
            debug!("route {name}: published {out}");
            true
        }
        Err(err) => {
            stats.errors.fetch_add(1, Ordering::Relaxed);
            warn!("route {name}: publish failed: {err}");
            false
        }
    }
}

/// True if a watched field differs enough from the last published state.
fn changed(
    state: &BTreeMap<String, Value>,
    last: Option<&BTreeMap<String, Value>>,
    on_change: &BTreeMap<String, crate::config::ChangeCfg>,
) -> bool {
    let Some(last) = last else {
        return !on_change.is_empty();
    };
    on_change.iter().any(|(field, cfg)| {
        let (now, before) = (state.get(field), last.get(field));
        match (
            now.and_then(Value::as_f64),
            before.and_then(Value::as_f64),
            cfg.min_delta,
        ) {
            (Some(a), Some(b), Some(delta)) => (a - b).abs() >= delta,
            _ => now != before,
        }
    })
}

#[allow(clippy::too_many_arguments)]
pub async fn run_state(
    name: String,
    mut rx: UnboundedReceiver<RouteInput>,
    inputs: Vec<StateInputCfg>,
    shared: SharedState,
    rules: StatePublishCfg,
    to: Endpoint,
    fields: BTreeMap<String, FieldSpec>,
    io: Arc<Io>,
    stats: Arc<RouteStats>,
) {
    let mut state = shared.lock().map(|s| s.clone()).unwrap_or_default();
    let min_interval = rules
        .max_rate_hz
        .filter(|r| *r > 0.0)
        .map(|r| Duration::from_secs_f64(1.0 / r))
        .unwrap_or(Duration::ZERO);
    let coalesce = Duration::from_millis(rules.coalesce_ms);
    let mut last_published: Option<BTreeMap<String, Value>> = None;
    let mut last_publish_at: Option<Instant> = None;
    let mut dirty_since: Option<Instant> = rules.on_start.then(Instant::now);
    let mut ticker = tokio::time::interval(TICK);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            msg = rx.recv() => {
                let Some(msg) = msg else { return };
                record_input(&stats, &msg.value);
                let input = &inputs[msg.input];
                for (field, spec) in &input.set {
                    let ctx = Context { input: Some(&msg.value), state: Some(&state) };
                    match spec.eval(&ctx) {
                        Ok(v) => { state.insert(field.clone(), v); }
                        Err(err) => {
                            stats.errors.fetch_add(1, Ordering::Relaxed);
                            warn!("route {name}: input {}: field {field}: {err}", input.name);
                        }
                    }
                }
                if let Ok(mut s) = shared.lock() {
                    *s = state.clone();
                }
                if input.publish || changed(&state, last_published.as_ref(), &rules.on_change) {
                    dirty_since.get_or_insert_with(Instant::now);
                }
            }
            _ = ticker.tick() => {}
        }

        let now = Instant::now();
        let due = dirty_since.is_some_and(|since| now - since >= coalesce)
            && last_publish_at.is_none_or(|at| now - at >= min_interval);
        if due && publish(&name, &io, &to, &fields, &state, &stats).await {
            last_published = Some(state.clone());
            last_publish_at = Some(now);
            dirty_since = None;
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn run_presence(
    name: String,
    mut rx: UnboundedReceiver<RouteInput>,
    silence: Duration,
    debounce: Duration,
    heartbeat: Duration,
    to: Endpoint,
    fields: BTreeMap<String, FieldSpec>,
    io: Arc<Io>,
    stats: Arc<RouteStats>,
) {
    let mut last_activity: Option<Instant> = None;
    let mut candidate = false;
    let mut candidate_since = Instant::now();
    let mut published: Option<bool> = None;
    let mut last_publish_at: Option<Instant> = None;
    let mut ticker = tokio::time::interval(Duration::from_millis(50));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            msg = rx.recv() => {
                let Some(msg) = msg else { return };
                record_input(&stats, &msg.value);
                last_activity = Some(Instant::now());
            }
            _ = ticker.tick() => {}
        }

        let now = Instant::now();
        let raw = last_activity.is_some_and(|t| now - t < silence);
        if raw != candidate {
            candidate = raw;
            candidate_since = now;
        }
        let stable = now - candidate_since >= debounce;
        let change = stable && published != Some(candidate);
        let beat = published.is_some() && last_publish_at.is_none_or(|t| now - t >= heartbeat);
        if change || beat {
            let state = BTreeMap::from([(
                "present".to_string(),
                Value::Bool(if change {
                    candidate
                } else {
                    published.unwrap_or(false)
                }),
            )]);
            if publish(&name, &io, &to, &fields, &state, &stats).await {
                if change {
                    info!("route {name}: present = {candidate}");
                    published = Some(candidate);
                }
                last_publish_at = Some(now);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ChangeCfg;
    use serde_json::json;

    fn st(pct: u64, alarm: bool) -> BTreeMap<String, Value> {
        BTreeMap::from([
            ("window_percentage".to_string(), json!(pct)),
            ("alarm_enabled".to_string(), json!(alarm)),
        ])
    }

    #[test]
    fn change_needs_one_point_or_alarm_flip() {
        let rules = BTreeMap::from([
            (
                "window_percentage".to_string(),
                ChangeCfg {
                    min_delta: Some(1.0),
                },
            ),
            ("alarm_enabled".to_string(), ChangeCfg::default()),
        ]);
        assert!(
            changed(&st(0, false), None, &rules),
            "nothing published yet"
        );
        assert!(!changed(&st(10, false), Some(&st(10, false)), &rules));
        assert!(changed(&st(11, false), Some(&st(10, false)), &rules));
        assert!(changed(&st(10, true), Some(&st(10, false)), &rules));
    }
}
