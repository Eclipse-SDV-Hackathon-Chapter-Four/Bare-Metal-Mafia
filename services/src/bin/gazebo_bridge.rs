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

//! `gazebo_bridge`: connects the Gazebo cabin simulation to the Guardian stack.
//!
//! The ROS side (`gazebo-sim/bridge/ros_zenoh_bridge.py`, inside the
//! gazebo-sim container) turns three ROS 2 topics into small JSON messages on
//! plain Zenoh keys and does the percent <-> metres conversion. This binary
//! maps those keys to uProtocol with the same up-rust stack as every other
//! service:
//!
//! ```text
//! gazebo/window/cmd       bridge -> gazebo-sim   {"percent": 25.0}   window setpoint
//! gazebo/window/position  gazebo-sim -> bridge   {"percent": 24.6}   measured glass position
//! gazebo/seat/contact     gazebo-sim -> bridge   {"contacts": 3}     only while something touches the row-2 cushion
//! ```
//!
//! `GAZEBO_MODE=mirror` (default): Gazebo follows `window-controller-sim`. The
//! bridge listens to `vss_window_state` and publishes nothing on uProtocol.
//!
//! `GAZEBO_MODE=replace`: Gazebo replaces `window-controller-sim` and
//! `child-presence-sim` (they must not run). The bridge consumes
//! `uds_window_cmd` / `uds_alarm_cmd` and publishes `vss_window_state`
//! (measured glass position) and `vss_child_presence` (seat contact,
//! debounced, with heartbeat).
//!
//! HTTP on `PORT` (default 8096): `GET /health`, `GET /state`.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use guardian_sil::{
    decode_json_payload, make_uri_provider, open_up_transport, open_zenoh_session,
    publish_json_event, uds_alarm_cmd_uri, uds_window_cmd_uri, vss_child_presence_uri,
    vss_window_state_uri, AlarmCommand, ChildPresenceEvent, WindowPositionCommand,
    WindowStateEvent,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::net::TcpListener;
use tracing::{info, warn};
use up_rust::{UListener, UMessage, UTransport};

const KEY_WINDOW_CMD: &str = "gazebo/window/cmd";
const KEY_WINDOW_POSITION: &str = "gazebo/window/position";
const KEY_SEAT_CONTACT: &str = "gazebo/seat/contact";

/// The setpoint is re-sent this often: uProtocol pub/sub keeps no last value,
/// so a lost message or a restarted gazebo-sim is corrected within a second.
const SETPOINT_REPEAT: Duration = Duration::from_secs(1);
/// Window state is published at most this often (5 Hz) while the glass moves.
const WINDOW_PUBLISH_PERIOD: Duration = Duration::from_millis(200);
/// Gazebo reports contacts only while something touches the cushion.
/// No contact message for this long means "seat empty".
const SEAT_SILENCE: Duration = Duration::from_millis(300);
/// A presence change must be stable this long before it is published.
const SEAT_DEBOUNCE: Duration = Duration::from_millis(400);
/// The current presence is repeated this often.
const PRESENCE_HEARTBEAT: Duration = Duration::from_secs(1);
/// /state reports gazebo_alive while position messages arrive this recently.
const GAZEBO_ALIVE: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Mode {
    Mirror,
    Replace,
}

#[derive(Default)]
struct Shared {
    /// Last commanded opening in percent (what Gazebo is told to do).
    setpoint: Option<u8>,
    /// Last measured opening in percent (what the Gazebo joint reports).
    measured: Option<f64>,
    measured_at: Option<Instant>,
    alarm_enabled: bool,
    /// Replace mode: a command arrived, publish the state even if unchanged
    /// (window-controller-sim published on every command, too).
    command_pending: bool,
    last_contact: Option<Instant>,
    /// Last published child presence (replace mode).
    child_present: Option<bool>,
}

impl Shared {
    fn window_percentage(&self) -> u8 {
        self.measured.map(percent_to_u8).unwrap_or(0)
    }
}

type SharedState = Arc<Mutex<Shared>>;

fn percent_to_u8(percent: f64) -> u8 {
    percent.round().clamp(0.0, 100.0) as u8
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Turns a stream of "something touches the seat" messages into debounced
/// presence events plus a heartbeat.
struct PresenceFilter {
    candidate: bool,
    candidate_since: Instant,
    published: Option<bool>,
    last_publish: Option<Instant>,
}

impl PresenceFilter {
    fn new(now: Instant) -> Self {
        Self {
            candidate: false,
            candidate_since: now,
            published: None,
            last_publish: None,
        }
    }

    /// Returns the value to publish now, if any.
    fn poll(&mut self, now: Instant, last_contact: Option<Instant>) -> Option<bool> {
        let raw = last_contact.is_some_and(|t| now.saturating_duration_since(t) < SEAT_SILENCE);
        if raw != self.candidate {
            self.candidate = raw;
            self.candidate_since = now;
        }
        let stable = now.saturating_duration_since(self.candidate_since) >= SEAT_DEBOUNCE;
        if stable && self.published != Some(self.candidate) {
            self.published = Some(self.candidate);
            self.last_publish = Some(now);
            return Some(self.candidate);
        }
        match (self.published, self.last_publish) {
            (Some(value), Some(at)) if now.saturating_duration_since(at) >= PRESENCE_HEARTBEAT => {
                self.last_publish = Some(now);
                Some(value)
            }
            _ => None,
        }
    }
}

async fn send_setpoint(zenoh: &zenoh::Session, percent: u8) {
    let payload = json!({ "percent": percent as f64 }).to_string();
    if let Err(err) = zenoh.put(KEY_WINDOW_CMD, payload).await {
        warn!("zenoh put {KEY_WINDOW_CMD} failed: {err}");
    }
}

/// Replace mode: uds/window/cmd and uds/alarm/cmd, as consumed by window-controller-sim.
struct CommandListener {
    shared: SharedState,
    zenoh: zenoh::Session,
}

#[async_trait]
impl UListener for CommandListener {
    async fn on_receive(&self, message: UMessage) {
        if let Ok(cmd) = decode_json_payload::<WindowPositionCommand>(&message) {
            let percent = cmd.percentage.min(100);
            info!("window command {percent}% (request {})", cmd.request_id);
            {
                let mut s = self.shared.lock().unwrap();
                s.setpoint = Some(percent);
                s.command_pending = true;
            }
            send_setpoint(&self.zenoh, percent).await;
        } else if let Ok(cmd) = decode_json_payload::<AlarmCommand>(&message) {
            info!("alarm command {} (request {})", cmd.enabled, cmd.request_id);
            let mut s = self.shared.lock().unwrap();
            s.alarm_enabled = cmd.enabled;
            s.command_pending = true;
        } else {
            warn!("ignored a command with an unknown payload");
        }
    }
}

/// Mirror mode: follow the window state published by window-controller-sim.
struct MirrorListener {
    shared: SharedState,
    zenoh: zenoh::Session,
}

#[async_trait]
impl UListener for MirrorListener {
    async fn on_receive(&self, message: UMessage) {
        match decode_json_payload::<WindowStateEvent>(&message) {
            Ok(event) => {
                let percent = event.window_percentage.min(100);
                let changed = {
                    let mut s = self.shared.lock().unwrap();
                    s.alarm_enabled = event.alarm_enabled;
                    s.setpoint.replace(percent) != Some(percent)
                };
                if changed {
                    info!("mirroring window state {percent}%");
                }
                send_setpoint(&self.zenoh, percent).await;
            }
            Err(err) => warn!("invalid window state payload: {err}"),
        }
    }
}

#[derive(Deserialize)]
struct PositionMsg {
    percent: f64,
}

async fn subscribe_gazebo(
    zenoh: &zenoh::Session,
    shared: SharedState,
) -> Result<(), zenoh::Error> {
    let s = shared.clone();
    zenoh
        .declare_subscriber(KEY_WINDOW_POSITION)
        .callback(move |sample| {
            match serde_json::from_slice::<PositionMsg>(&sample.payload().to_bytes()) {
                Ok(msg) => {
                    let mut s = s.lock().unwrap();
                    s.measured = Some(msg.percent);
                    s.measured_at = Some(Instant::now());
                }
                Err(err) => warn!("invalid JSON on {KEY_WINDOW_POSITION}: {err}"),
            }
        })
        .background()
        .await?;

    zenoh
        .declare_subscriber(KEY_SEAT_CONTACT)
        .callback(move |_sample| {
            shared.lock().unwrap().last_contact = Some(Instant::now());
        })
        .background()
        .await?;
    Ok(())
}

/// Re-send the last setpoint every second (both modes).
fn spawn_setpoint_repeat(shared: SharedState, zenoh: zenoh::Session) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(SETPOINT_REPEAT);
        loop {
            tick.tick().await;
            let setpoint = shared.lock().unwrap().setpoint;
            if let Some(percent) = setpoint {
                send_setpoint(&zenoh, percent).await;
            }
        }
    });
}

/// Replace mode: the only task that publishes the window state, so events
/// leave in order. Publishes on start, on every command, and while the glass
/// moves (each whole-percent change, at most 5 Hz; the final value always
/// goes out on the next tick).
fn spawn_window_state(shared: SharedState, transport: Arc<dyn UTransport>) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(WINDOW_PUBLISH_PERIOD);
        let mut last: Option<(u8, bool)> = None;
        loop {
            tick.tick().await;
            let (current, forced) = {
                let mut s = shared.lock().unwrap();
                let forced = std::mem::take(&mut s.command_pending);
                ((s.window_percentage(), s.alarm_enabled), forced)
            };
            if !forced && last == Some(current) {
                continue;
            }
            let event = WindowStateEvent {
                window_percentage: current.0,
                alarm_enabled: current.1,
                timestamp_ms: now_ms(),
            };
            match publish_json_event(transport.clone(), vss_window_state_uri(), &event).await {
                Ok(()) => last = Some(current),
                Err(err) => warn!("publishing window state failed: {err}"),
            }
        }
    });
}

/// Replace mode: seat contact -> ChildPresenceEvent.
fn spawn_presence(shared: SharedState, transport: Arc<dyn UTransport>) {
    tokio::spawn(async move {
        let mut filter = PresenceFilter::new(Instant::now());
        let mut tick = tokio::time::interval(Duration::from_millis(50));
        loop {
            tick.tick().await;
            let last_contact = shared.lock().unwrap().last_contact;
            let Some(present) = filter.poll(Instant::now(), last_contact) else {
                continue;
            };
            if shared.lock().unwrap().child_present.replace(present) != Some(present) {
                info!("child present = {present}");
            }
            let event = ChildPresenceEvent {
                present,
                // The simulated contact sensor has no confidence of its own;
                // in simulation it is ground truth.
                confidence: 1.0,
                zone: Some("rear_center".to_string()),
                timestamp_ms: now_ms(),
            };
            if let Err(err) =
                publish_json_event(transport.clone(), vss_child_presence_uri(), &event).await
            {
                warn!("publishing child presence failed: {err}");
            }
        }
    });
}

#[derive(Clone)]
struct HttpState {
    mode: Mode,
    shared: SharedState,
}

async fn get_state(State(app): State<HttpState>) -> Json<serde_json::Value> {
    let s = app.shared.lock().unwrap();
    let now = Instant::now();
    Json(json!({
        "mode": app.mode,
        "gazebo_alive": s.measured_at.is_some_and(|t| now.saturating_duration_since(t) < GAZEBO_ALIVE),
        "setpoint_percent": s.setpoint,
        "window_percentage": s.measured.map(percent_to_u8),
        "alarm_enabled": s.alarm_enabled,
        "seat_contact": s.last_contact.is_some_and(|t| now.saturating_duration_since(t) < SEAT_SILENCE),
        "child_present": s.child_present,
    }))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "gazebo_bridge=info,info".to_string()),
        )
        .init();

    let mode = match std::env::var("GAZEBO_MODE").as_deref() {
        Ok("replace") => Mode::Replace,
        Ok("mirror") | Err(_) => Mode::Mirror,
        Ok(other) => return Err(format!("GAZEBO_MODE={other}: expected mirror or replace").into()),
    };
    let port = std::env::var("PORT").unwrap_or_else(|_| "8096".to_string());

    let transport = open_up_transport(make_uri_provider("gazebo-bridge", 0x9602, 0x01)).await?;
    let zenoh = open_zenoh_session()
        .await
        .map_err(|e| format!("zenoh open failed: {e}"))?;
    let shared: SharedState = Arc::new(Mutex::new(Shared::default()));

    subscribe_gazebo(&zenoh, shared.clone())
        .await
        .map_err(|e| format!("zenoh subscribe failed: {e}"))?;
    spawn_setpoint_repeat(shared.clone(), zenoh.clone());

    match mode {
        Mode::Mirror => {
            transport
                .register_listener(
                    &vss_window_state_uri(),
                    None,
                    Arc::new(MirrorListener {
                        shared: shared.clone(),
                        zenoh: zenoh.clone(),
                    }),
                )
                .await?;
        }
        Mode::Replace => {
            let listener = Arc::new(CommandListener {
                shared: shared.clone(),
                zenoh: zenoh.clone(),
            });
            transport
                .register_listener(&uds_window_cmd_uri(), None, listener.clone())
                .await?;
            transport
                .register_listener(&uds_alarm_cmd_uri(), None, listener)
                .await?;
            spawn_window_state(shared.clone(), transport.clone());
            spawn_presence(shared.clone(), transport.clone());
        }
    }

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/state", get(get_state))
        .with_state(HttpState { mode, shared });
    let addr = format!("0.0.0.0:{port}");
    info!("gazebo_bridge mode {mode:?}, HTTP on {addr}");
    axum::serve(TcpListener::bind(&addr).await?, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn percent_rounds_and_clamps() {
        assert_eq!(percent_to_u8(24.6), 25);
        assert_eq!(percent_to_u8(-3.0), 0);
        assert_eq!(percent_to_u8(140.0), 100);
    }

    #[test]
    fn empty_seat_is_published_after_debounce() {
        let t0 = Instant::now();
        let mut f = PresenceFilter::new(t0);
        assert_eq!(f.poll(t0 + ms(100), None), None);
        assert_eq!(f.poll(t0 + ms(400), None), Some(false));
        assert_eq!(f.poll(t0 + ms(900), None), None);
        assert_eq!(f.poll(t0 + ms(1400), None), Some(false)); // heartbeat
    }

    #[test]
    fn contact_becomes_present_after_debounce_and_absent_after_silence() {
        let t0 = Instant::now();
        let mut f = PresenceFilter::new(t0);
        assert_eq!(f.poll(t0 + ms(400), None), Some(false));
        // contacts arrive continuously from t = 1000 ms to t = 3600 ms
        let contact = |t: u64| Some(t0 + ms(t.clamp(1000, 3600)));
        assert_eq!(f.poll(t0 + ms(1000), contact(1000)), None);
        assert_eq!(f.poll(t0 + ms(1350), contact(1350)), None);
        assert_eq!(f.poll(t0 + ms(1400), contact(1400)), Some(true));
        assert_eq!(f.poll(t0 + ms(3600), contact(3600)), Some(true)); // heartbeat
        assert_eq!(f.poll(t0 + ms(3900), contact(3900)), None); // silence starts
        assert_eq!(f.poll(t0 + ms(4250), contact(4250)), None); // still debouncing
        assert_eq!(f.poll(t0 + ms(4300), contact(4300)), Some(false));
    }

    #[test]
    fn short_contact_is_ignored() {
        let t0 = Instant::now();
        let mut f = PresenceFilter::new(t0);
        assert_eq!(f.poll(t0 + ms(400), None), Some(false));
        let blip = Some(t0 + ms(1000));
        assert_eq!(f.poll(t0 + ms(1000), blip), None);
        assert_eq!(f.poll(t0 + ms(1300), blip), None); // silent again
        assert_eq!(f.poll(t0 + ms(1350), blip), None);
        // never stable for 400 ms -> no "true" event, only the heartbeat
        assert_eq!(f.poll(t0 + ms(1400), blip), Some(false));
    }
}
