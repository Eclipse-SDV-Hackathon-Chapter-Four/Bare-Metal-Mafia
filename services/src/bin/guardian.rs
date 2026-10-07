use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use futures_util::StreamExt;
use guardian_sil::{
    decode_json_payload, evaluate_state, hvac_state_uri, make_uri_provider, mitigation_rpc_uri,
    open_up_transport, publish_json_event, vss_cabin_temperature_uri, vss_child_presence_uri,
    vss_guardian_state_uri, CabinTemperatureEvent, ChildPresenceEvent, GuardianSnapshot,
    GuardianState, HvacStateEvent, MitigationRequest, SensorStatus,
};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tracing::{info, warn};
use up_rust::communication::{CallOptions, InMemoryRpcClient, RpcClient};
use up_rust::{UListener, UMessage, UTransport, UPayloadFormat};

const EWS_PORT: u16 = 8765;
const EWS_STATE_BROADCAST_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone)]
struct AppState {
    data: Arc<Mutex<GuardianRuntime>>,
}

#[derive(Debug)]
struct GuardianRuntime {
    child_present: bool,
    temperature_celsius: f32,
    current_state: GuardianState,
    ews_warn: Arc<AtomicBool>,
    hvac_active: bool,
    hvac_fault_active: bool,
    hvac_target_temperature_celsius: i8,
    hvac_request_started_ms: Option<u64>,
    hvac_stage_requested: bool,
    window_stage_requested: bool,
    mitigation_pending: bool,
}

#[derive(serde::Serialize)]
struct EwsGuardianState {
    time: u64,
    temperature: f32,
    child_presence: bool,
    hvac_active: bool,
    windows_down: bool,
    state: GuardianState,
}

#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct EwsWarn {
    time: u64,
    reason: EwsReason,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
enum EwsReason {
    Heat,
}

struct ChildPresenceListener {
    app: AppState,
    transport: Arc<dyn UTransport>,
    rpc_client: Arc<InMemoryRpcClient>,
}

#[async_trait]
impl UListener for ChildPresenceListener {
    async fn on_receive(&self, message: UMessage) {
        match decode_json_payload::<ChildPresenceEvent>(&message) {
            Ok(event) => {
                let (should_trigger, snapshot) = {
                    let mut guard = self.app.data.lock().await;
                    let trigger = guard.apply_child_event(event);
                    (trigger, guard.snapshot())
                };

                let _ = publish_json_event(
                    self.transport.clone(),
                    vss_guardian_state_uri(),
                    &snapshot,
                )
                .await;

                if should_trigger {
                    request_mitigation(&self.app, self.rpc_client.clone()).await;
                }
            }
            Err(err) => warn!("Invalid child event payload: {}", err),
        }
    }
}

struct TemperatureListener {
    app: AppState,
    transport: Arc<dyn UTransport>,
    rpc_client: Arc<InMemoryRpcClient>,
}

#[async_trait]
impl UListener for TemperatureListener {
    async fn on_receive(&self, message: UMessage) {
        match decode_json_payload::<CabinTemperatureEvent>(&message) {
            Ok(event) => {
                let (should_trigger, snapshot) = {
                    let mut guard = self.app.data.lock().await;
                    let trigger = guard.apply_temperature_event(event);
                    (trigger, guard.snapshot())
                };

                let _ = publish_json_event(
                    self.transport.clone(),
                    vss_guardian_state_uri(),
                    &snapshot,
                )
                .await;

                if should_trigger {
                    request_mitigation(&self.app, self.rpc_client.clone()).await;
                }
            }
            Err(err) => warn!("Invalid temperature event payload: {}", err),
        }
    }
}

struct HvacStateListener {
    app: AppState,
    transport: Arc<dyn UTransport>,
    rpc_client: Arc<InMemoryRpcClient>,
}

#[async_trait]
impl UListener for HvacStateListener {
    async fn on_receive(&self, message: UMessage) {
        match decode_json_payload::<HvacStateEvent>(&message) {
            Ok(event) => {
                let (should_trigger, snapshot) = {
                    let mut guard = self.app.data.lock().await;
                    let trigger = guard.apply_hvac_state_event(event);
                    (trigger, guard.snapshot())
                };

                let _ = publish_json_event(
                    self.transport.clone(),
                    vss_guardian_state_uri(),
                    &snapshot,
                )
                .await;

                if should_trigger {
                    request_mitigation(&self.app, self.rpc_client.clone()).await;
                }
            }
            Err(err) => warn!("Invalid HVAC state payload: {}", err),
        }
    }
}

impl GuardianRuntime {
    fn new() -> Self {
        Self {
            child_present: false,
            temperature_celsius: 26.0,
            current_state: GuardianState::Clear,
            ews_warn: Arc::new(AtomicBool::new(false)),
            hvac_active: false,
            hvac_fault_active: false,
            hvac_target_temperature_celsius: 22,
            hvac_request_started_ms: None,
            hvac_stage_requested: false,
            window_stage_requested: false,
            mitigation_pending: false,
        }
    }

    fn apply_child_event(&mut self, event: ChildPresenceEvent) -> bool {
        self.child_present = event.present;
        self.recompute_and_log()
    }

    fn apply_temperature_event(&mut self, event: CabinTemperatureEvent) -> bool {
        if matches!(event.sensor_status, SensorStatus::Ok | SensorStatus::Degraded) {
            self.temperature_celsius = event.temperature_celsius;
            return self.recompute_and_log();
        }

        false
    }

    fn recompute_and_log(&mut self) -> bool {
        let base = evaluate_state(
            self.child_present,
            self.temperature_celsius,
            self.ews_warn.load(Ordering::Relaxed),
        );
        let mut trigger_mitigation = false;

        if base == GuardianState::Critical {
            self.current_state = if self.window_stage_requested || self.hvac_active {
                GuardianState::Mitigating
            } else {
                GuardianState::Critical
            };

            if !self.window_stage_requested {
                if self.hvac_fault_active {
                    if !self.mitigation_pending {
                        trigger_mitigation = true;
                    }
                } else if !self.hvac_stage_requested {
                    if !self.mitigation_pending {
                        trigger_mitigation = true;
                    }
                } else if self.should_escalate_to_window() && !self.mitigation_pending {
                    trigger_mitigation = true;
                }
            }
        } else {
            self.current_state = base;
            self.mitigation_pending = false;
            self.window_stage_requested = false;
            self.hvac_stage_requested = false;
            self.hvac_request_started_ms = None;
        }

        info!(
            "Child: {} | Temperature: {:.1}C | HVAC active={} target={}C fault={} | stages hvac={} window={} -> {:?}",
            self.child_present,
            self.temperature_celsius,
            self.hvac_active,
            self.hvac_target_temperature_celsius,
            self.hvac_fault_active,
            self.hvac_stage_requested,
            self.window_stage_requested,
            self.current_state
        );

        trigger_mitigation
    }

    fn apply_hvac_state_event(&mut self, event: HvacStateEvent) -> bool {
        self.hvac_active = event.air_conditioning_active;
        self.hvac_fault_active = event.fault_active;
        self.hvac_target_temperature_celsius = event.target_temperature_celsius;

        if self.hvac_fault_active && self.current_state == GuardianState::Critical && !self.window_stage_requested {
            return true;
        }

        self.recompute_and_log()
    }

    fn should_escalate_to_window(&self) -> bool {
        if self.window_stage_requested || self.hvac_fault_active {
            return true;
        }

        match self.hvac_request_started_ms {
            Some(started_ms) => now_ms().saturating_sub(started_ms) >= 12_000,
            None => false,
        }
    }

    fn mark_mitigation_success(&mut self, request: &MitigationRequest) {
        self.mitigation_pending = false;
        self.hvac_stage_requested = request.hvac_power_enabled;
        self.window_stage_requested = request.window_percentage > 0;
        if request.hvac_power_enabled {
            self.hvac_request_started_ms = Some(now_ms());
            self.hvac_target_temperature_celsius = request.hvac_target_temperature_celsius;
        }
        self.current_state = GuardianState::Mitigating;
        info!(
            "Mitigation RPC accepted -> hvac_target={}C window={} alarm={}",
            request.hvac_target_temperature_celsius, request.window_percentage, request.alarm_enabled
        );
    }

    fn mark_mitigation_requested(&mut self) {
        self.mitigation_pending = true;
    }

    fn mark_mitigation_failed(&mut self) {
        self.mitigation_pending = false;
    }

    fn snapshot(&self) -> GuardianSnapshot {
        GuardianSnapshot {
            state: self.current_state,
            child_present: self.child_present,
            temperature_celsius: self.temperature_celsius,
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "guardian=info,info".to_string()),
        )
        .init();

    let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());
    let addr = format!("{}:{}", host, port);
    let ews_addr = format!("{}:{}", host, EWS_PORT);

    let uri_provider = make_uri_provider("guardian", 0x1001, 0x01);
    let transport = open_up_transport(uri_provider.clone()).await?;
    let rpc_client = Arc::new(InMemoryRpcClient::new(transport.clone(), uri_provider).await?);

    let runtime = GuardianRuntime::new();
    info!("Guardian boot at {}", now_ms());
    info!(
        "Child: {} | Temperature: {:.1}C -> {:?}",
        runtime.child_present, runtime.temperature_celsius, runtime.current_state
    );

    let app_state = AppState {
        data: Arc::new(Mutex::new(runtime)),
    };

    transport
        .register_listener(
            &vss_child_presence_uri(),
            None,
            Arc::new(ChildPresenceListener {
                app: app_state.clone(),
                transport: transport.clone(),
                rpc_client: rpc_client.clone(),
            }),
        )
        .await?;

    transport
        .register_listener(
            &vss_cabin_temperature_uri(),
            None,
            Arc::new(TemperatureListener {
                app: app_state.clone(),
                transport: transport.clone(),
                rpc_client: rpc_client.clone(),
            }),
        )
        .await?;

    transport
        .register_listener(
            &hvac_state_uri(),
            None,
            Arc::new(HvacStateListener {
                app: app_state.clone(),
                transport: transport.clone(),
                rpc_client: rpc_client.clone(),
            }),
        )
        .await?;

    let app = Router::new()
        .route("/health", get(health))
        .route("/state", get(get_state))
        .with_state(app_state.clone());

    let listener = TcpListener::bind(&addr).await?;
    info!("Guardian listening on {}", addr);

    let ews_app = Router::new()
        .route("/ws", get(ews_websocket))
        .with_state(app_state);
    let ews_listener = TcpListener::bind(&ews_addr).await?;
    info!("Guardian EWS WebSocket listening on ws://{}/ws", ews_addr);

    tokio::try_join!(
        axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()),
        axum::serve(ews_listener, ews_app).with_graceful_shutdown(shutdown_signal()),
    )?;

    Ok(())
}

async fn health() -> StatusCode {
    StatusCode::OK
}

async fn get_state(State(app): State<AppState>) -> Json<GuardianSnapshot> {
    let guard = app.data.lock().await;
    Json(guard.snapshot())
}

async fn ews_websocket(
    State(app): State<AppState>,
    websocket: WebSocketUpgrade,
) -> axum::response::Response {
    let ews_warn = app.data.lock().await.ews_warn.clone();
    websocket.on_upgrade(move |socket| handle_ews_socket(socket, app, ews_warn))
}

async fn handle_ews_socket(mut socket: WebSocket, app: AppState, ews_warn: Arc<AtomicBool>) {
    let mut broadcast_interval = tokio::time::interval(EWS_STATE_BROADCAST_INTERVAL);

    loop {
        tokio::select! {
            _ = broadcast_interval.tick() => {
                let state = {
                    let guard = app.data.lock().await;
                    EwsGuardianState {
                        time: now_ms(),
                        temperature: guard.temperature_celsius,
                        child_presence: guard.child_present,
                        hvac_active: guard.hvac_active,
                        windows_down: guard.window_stage_requested,
                        state: guard.current_state,
                    }
                };

                let payload = match serde_json::to_string(&state) {
                    Ok(payload) => payload,
                    Err(err) => {
                        warn!("EWS state serialization failed: {}", err);
                        continue;
                    }
                };

                if socket.send(Message::Text(payload)).await.is_err() {
                    break;
                }
            }
            message = socket.next() => {
                match message {
                    Some(Ok(Message::Text(payload))) => {
                        match serde_json::from_str::<EwsWarn>(&payload) {
                            Ok(warning) => handle_ews_warning(warning, ews_warn.clone()),
                            Err(err) => warn!("Invalid EWS warning payload: {}", err),
                        }
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        if socket.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(Message::Binary(_))) | Some(Ok(Message::Pong(_))) => {}
                }
            }
        }
    }
}

fn handle_ews_warning(warning: EwsWarn, ews_warn: Arc<AtomicBool>) {
    match warning.reason {
        EwsReason::Heat => ews_warn.store(true, Ordering::Relaxed),
    }
}

async fn request_mitigation(app: &AppState, rpc_client: Arc<InMemoryRpcClient>) {
    let request = {
        let mut guard = app.data.lock().await;
        guard.mark_mitigation_requested();
        build_mitigation_request(&guard)
    };

    let payload = match serde_json::to_vec(&request) {
        Ok(bytes) => up_rust::communication::UPayload::new(bytes, UPayloadFormat::UPAYLOAD_FORMAT_JSON),
        Err(err) => {
            warn!("Mitigation request serialization failed: {}", err);
            let mut guard = app.data.lock().await;
            guard.mark_mitigation_failed();
            return;
        }
    };

    let result = rpc_client
        .invoke_method(
            mitigation_rpc_uri(),
            CallOptions::for_rpc_request(5_000, None, None, None),
            Some(payload),
        )
        .await;

    let mut guard = app.data.lock().await;
    match result {
        Ok(_) => guard.mark_mitigation_success(&request),
        Err(err) => {
            guard.mark_mitigation_failed();
            warn!("Mitigation RPC failed: {}", err);
        }
    }
}

fn build_mitigation_request(guard: &GuardianRuntime) -> MitigationRequest {
    let escalate_to_window = guard.hvac_fault_active || guard.should_escalate_to_window();

    if escalate_to_window {
        MitigationRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            hvac_target_temperature_celsius: 18,
            hvac_fan_speed_percent: 100,
            hvac_power_enabled: !guard.hvac_fault_active,
            window_percentage: 25,
            fan_enabled: true,
            alarm_enabled: true,
            reason: if guard.hvac_fault_active {
                "CHILD_HAZARD_CRITICAL_HVAC_FAULT".to_string()
            } else {
                "CHILD_HAZARD_CRITICAL_ESCALATE_WINDOW".to_string()
            },
            timestamp_ms: now_ms(),
        }
    } else {
        MitigationRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            hvac_target_temperature_celsius: 18,
            hvac_fan_speed_percent: 100,
            hvac_power_enabled: true,
            window_percentage: 0,
            fan_enabled: true,
            alarm_enabled: false,
            reason: "CHILD_HAZARD_CRITICAL_HVAC_FIRST".to_string(),
            timestamp_ms: now_ms(),
        }
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    tokio::select! {
        _ = ctrl_c => {},
        _ = tokio::time::sleep(Duration::from_secs(u64::MAX)) => {},
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
