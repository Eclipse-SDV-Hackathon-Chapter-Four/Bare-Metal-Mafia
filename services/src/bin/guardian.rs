use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use guardian_sil::{
    decode_json_payload, evaluate_state, make_uri_provider, mitigation_rpc_uri, open_up_transport,
    publish_json_event, vss_cabin_temperature_uri, vss_child_presence_uri, vss_guardian_state_uri,
    CabinTemperatureEvent, ChildPresenceEvent, GuardianSnapshot, GuardianState, MitigationRequest,
    SensorStatus,
};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tracing::{info, warn};
use up_rust::communication::{CallOptions, InMemoryRpcClient, RpcClient};
use up_rust::{UListener, UMessage, UTransport, UPayloadFormat};

#[derive(Clone)]
struct AppState {
    data: Arc<Mutex<GuardianRuntime>>,
}

#[derive(Debug)]
struct GuardianRuntime {
    child_present: bool,
    temperature_celsius: f32,
    current_state: GuardianState,
    mitigation_active: bool,
    mitigation_pending: bool,
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

impl GuardianRuntime {
    fn new() -> Self {
        Self {
            child_present: false,
            temperature_celsius: 26.0,
            current_state: GuardianState::Clear,
            mitigation_active: false,
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
        let base = evaluate_state(self.child_present, self.temperature_celsius);
        let mut trigger_mitigation = false;

        if base == GuardianState::Critical {
            if self.mitigation_active {
                self.current_state = GuardianState::Mitigating;
            } else if self.mitigation_pending {
                self.current_state = GuardianState::Critical;
            } else {
                self.current_state = GuardianState::Critical;
                trigger_mitigation = true;
            }
        } else {
            self.current_state = base;
            self.mitigation_active = false;
            self.mitigation_pending = false;
        }

        info!(
            "Child: {} | Temperature: {:.1}C -> {:?}",
            self.child_present, self.temperature_celsius, self.current_state
        );

        trigger_mitigation
    }

    fn mark_mitigation_success(&mut self) {
        self.mitigation_active = true;
        self.mitigation_pending = false;
        self.current_state = GuardianState::Mitigating;
        info!("Mitigation RPC accepted -> {:?}", self.current_state);
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

    let app = Router::new()
        .route("/health", get(health))
        .route("/state", get(get_state))
        .with_state(app_state);

    let listener = TcpListener::bind(&addr).await?;
    info!("Guardian listening on {}", addr);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn health() -> StatusCode {
    StatusCode::OK
}

async fn get_state(State(app): State<AppState>) -> Json<GuardianSnapshot> {
    let guard = app.data.lock().await;
    Json(guard.snapshot())
}

async fn request_mitigation(app: &AppState, rpc_client: Arc<InMemoryRpcClient>) {
    {
        let mut guard = app.data.lock().await;
        guard.mark_mitigation_requested();
    }

    let request = MitigationRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        window_percentage: 25,
        fan_enabled: true,
        alarm_enabled: true,
        reason: "CHILD_HAZARD_CRITICAL".to_string(),
        timestamp_ms: now_ms(),
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
        Ok(_) => guard.mark_mitigation_success(),
        Err(err) => {
            guard.mark_mitigation_failed();
            warn!("Mitigation RPC failed: {}", err);
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
