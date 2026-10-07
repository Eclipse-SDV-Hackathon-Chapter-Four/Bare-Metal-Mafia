use std::sync::Arc;

use async_trait::async_trait;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use guardian_sil::{
    decode_json_payload, make_uri_provider, open_up_transport, publish_json_event,
    uds_alarm_cmd_uri, uds_window_cmd_uri, vss_window_state_uri, AlarmCommand,
    WindowPositionCommand, WindowStateEvent,
};
use serde::Serialize;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, Notify};
use tracing::{info, warn};
use up_rust::{UListener, UMessage};

// Assisted-by: Anthropic Claude Fable 5.1 (claude-fable-5-1)
/// Changes arriving within this window are published as one state event.
const PUBLISH_COALESCE: std::time::Duration = std::time::Duration::from_millis(20);

#[derive(Debug, Default)]
struct ControllerState {
    window_percentage: u8,
    alarm_enabled: bool,
}

#[derive(Clone)]
struct AppState {
    state: Arc<Mutex<ControllerState>>,
}

#[derive(Serialize)]
struct ControllerSnapshot {
    window_percentage: u8,
    alarm_enabled: bool,
}

struct UdsWindowListener {
    state: Arc<Mutex<ControllerState>>,
    publish: Arc<Notify>,
}

#[async_trait]
impl UListener for UdsWindowListener {
    async fn on_receive(&self, message: UMessage) {
        match decode_json_payload::<WindowPositionCommand>(&message) {
            Ok(cmd) => {
                {
                    let mut guard = self.state.lock().await;
                    guard.window_percentage = cmd.percentage.min(100);
                    info!(
                        "UDS window write -> {}% request_id={}",
                        guard.window_percentage, cmd.request_id
                    );
                }
                // Assisted-by: Anthropic Claude Fable 5.1 (claude-fable-5-1)
                self.publish.notify_one();
            }
            Err(err) => warn!("Invalid UDS window payload: {}", err),
        }
    }
}

struct UdsAlarmListener {
    state: Arc<Mutex<ControllerState>>,
    publish: Arc<Notify>,
}

#[async_trait]
impl UListener for UdsAlarmListener {
    async fn on_receive(&self, message: UMessage) {
        match decode_json_payload::<AlarmCommand>(&message) {
            Ok(cmd) => {
                {
                    let mut guard = self.state.lock().await;
                    guard.alarm_enabled = cmd.enabled;
                    info!(
                        "UDS alarm write -> {} request_id={}",
                        guard.alarm_enabled, cmd.request_id
                    );
                }
                // Assisted-by: Anthropic Claude Fable 5.1 (claude-fable-5-1)
                self.publish.notify_one();
            }
            Err(err) => warn!("Invalid UDS alarm payload: {}", err),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_env_filter(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "window_controller_sim=info,info".to_string()),
        )
        .init();

    let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port = std::env::var("PORT").unwrap_or_else(|_| "8092".to_string());
    let addr = format!("{}:{}", host, port);

    let app_state = AppState {
        state: Arc::new(Mutex::new(ControllerState::default())),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/state", get(get_state))
        .with_state(app_state.clone());

    let uri_provider = make_uri_provider("window-controller-sim", 0x9401, 0x01);
    let transport = open_up_transport(uri_provider).await?;

    publish_state(transport.clone(), &app_state.state).await;

    // Assisted-by: Anthropic Claude Fable 5.1 (claude-fable-5-1)
    // All later state events come from this one task. up-transport-zenoh runs
    // every listener call in its own tokio task, so two events published
    // back to back (e.g. after a window and an alarm command that arrive
    // together) can reach any subscriber in either order, and the stale one
    // would win. Coalescing changes for PUBLISH_COALESCE and publishing once,
    // sequentially, leaves the final state as the last event on the bus.
    let publish = Arc::new(Notify::new());
    {
        let (transport, state, publish) =
            (transport.clone(), app_state.state.clone(), publish.clone());
        tokio::spawn(async move {
            loop {
                publish.notified().await;
                tokio::time::sleep(PUBLISH_COALESCE).await;
                publish_state(transport.clone(), &state).await;
            }
        });
    }

    transport
        .register_listener(
            &uds_window_cmd_uri(),
            None,
            Arc::new(UdsWindowListener {
                state: app_state.state.clone(),
                publish: publish.clone(),
            }),
        )
        .await?;

    transport
        .register_listener(
            &uds_alarm_cmd_uri(),
            None,
            Arc::new(UdsAlarmListener {
                state: app_state.state.clone(),
                publish: publish.clone(),
            }),
        )
        .await?;

    let listener = TcpListener::bind(&addr).await?;
    info!("Window controller sim listening on {}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> StatusCode {
    StatusCode::OK
}

async fn get_state(State(app): State<AppState>) -> Json<ControllerSnapshot> {
    let guard = app.state.lock().await;
    Json(ControllerSnapshot {
        window_percentage: guard.window_percentage,
        alarm_enabled: guard.alarm_enabled,
    })
}

async fn publish_state(
    transport: Arc<dyn up_rust::UTransport>,
    state: &Arc<Mutex<ControllerState>>,
) {
    let snapshot = {
        let guard = state.lock().await;
        WindowStateEvent {
            window_percentage: guard.window_percentage,
            alarm_enabled: guard.alarm_enabled,
            timestamp_ms: now_ms(),
        }
    };

    if let Err(err) = publish_json_event(transport, vss_window_state_uri(), &snapshot).await {
        warn!("Failed to publish window state: {}", err);
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
