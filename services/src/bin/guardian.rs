use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
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
use up_rust::{UListener, UMessage, UPayloadFormat, UTransport};

const SENSOR_DISAGREEMENT_THRESHOLD_CELSIUS: f32 = 5.0;

#[derive(Clone)]
struct AppState {
    data: Arc<Mutex<GuardianRuntime>>,
}

#[derive(Debug)]
struct GuardianRuntime {
    child_present: bool,
    temperature_celsius: f32,
    known_temperature_sensors: HashSet<u64>,
    sensor_temperatures: HashMap<u64, f32>,
    broken_sensors: HashSet<u64>,
    current_state: GuardianState,
    hvac_active: bool,
    hvac_fault_active: bool,
    hvac_target_temperature_celsius: i8,
    hvac_request_started_ms: Option<u64>,
    hvac_stage_requested: bool,
    window_stage_requested: bool,
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
            known_temperature_sensors: HashSet::new(),
            sensor_temperatures: HashMap::new(),
            broken_sensors: HashSet::new(),
            current_state: GuardianState::Clear,
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
        self.known_temperature_sensors.insert(event.sensor_id);
        if self.broken_sensors.contains(&event.sensor_id)
        {
            return false;
        }

        if matches!(event.sensor_status, SensorStatus::Failed) {
            self.broken_sensors.insert(event.sensor_id);
            self.sensor_temperatures.remove(&event.sensor_id);
            warn!("Temperature sensor {} reported failure", event.sensor_id);
            if self.all_temperature_sensors_broken() {
                return self.recompute_and_log();
            }
            return false;
        }

        if !matches!(event.sensor_status, SensorStatus::Ok | SensorStatus::Degraded) {
            return false;
        }

        let disagreeing_sensors: Vec<u64> = self
            .sensor_temperatures
            .iter()
            .filter_map(|(&sensor_id, &temperature)| {
                (sensor_id != event.sensor_id
                    && (temperature - event.temperature_celsius).abs()
                        > SENSOR_DISAGREEMENT_THRESHOLD_CELSIUS)
                    .then_some(sensor_id)
            })
            .collect();

        if !disagreeing_sensors.is_empty() {
            self.broken_sensors.insert(event.sensor_id);
            self.sensor_temperatures.remove(&event.sensor_id);
            for sensor_id in disagreeing_sensors {
                self.broken_sensors.insert(sensor_id);
                self.sensor_temperatures.remove(&sensor_id);
                warn!(
                    "Temperature sensors {} and {} disagree by more than {:.1}C; marking both broken",
                    event.sensor_id,
                    sensor_id,
                    SENSOR_DISAGREEMENT_THRESHOLD_CELSIUS
                );
            }
            if self.sensor_temperatures.is_empty() {
                return self.recompute_and_log();
            }

            self.temperature_celsius = self.sensor_temperatures.values().sum::<f32>()
                / self.sensor_temperatures.len() as f32;
            return self.recompute_and_log();
        }

        self.sensor_temperatures
            .insert(event.sensor_id, event.temperature_celsius);
        self.temperature_celsius =
            self.sensor_temperatures.values().sum::<f32>() / self.sensor_temperatures.len() as f32;
        self.recompute_and_log()
    }

    fn all_temperature_sensors_broken(&self) -> bool {
        !self.known_temperature_sensors.is_empty()
            && self
                .known_temperature_sensors
                .iter()
                .all(|sensor_id| self.broken_sensors.contains(sensor_id))
    }

    fn recompute_and_log(&mut self) -> bool {
        if self.all_temperature_sensors_broken() {
            self.current_state = GuardianState::Critical;
            info!(
                "All {} observed temperature sensors are broken -> {:?}",
                self.known_temperature_sensors.len(),
                self.current_state
            );
            return !self.mitigation_pending && !self.window_stage_requested;
        }

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
    if guard.all_temperature_sensors_broken() {
        return MitigationRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            hvac_target_temperature_celsius: 18,
            hvac_fan_speed_percent: 100,
            hvac_power_enabled: !guard.hvac_fault_active,
            window_percentage: 25,
            fan_enabled: true,
            alarm_enabled: true,
            reason: "TEMPERATURE_SENSOR_FAILURE_ALL".to_string(),
            timestamp_ms: now_ms(),
        };
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    fn temperature_event(sensor_id: u64, temperature_celsius: f32) -> CabinTemperatureEvent {
        CabinTemperatureEvent {
            temperature_celsius,
            timestamp_ms: 0,
            sensor_status: SensorStatus::Ok,
            sensor_id,
        }
    }

    #[test]
    fn agreeing_sensors_are_averaged() {
        let mut runtime = GuardianRuntime::new();
        runtime.apply_temperature_event(temperature_event(1, 30.0));
        runtime.apply_temperature_event(temperature_event(2, 32.0));

        assert_eq!(runtime.temperature_celsius, 31.0);
        assert!(runtime.broken_sensors.is_empty());
    }

    #[test]
    fn divergent_sensors_are_both_marked_broken_and_ignored() {
        let mut runtime = GuardianRuntime::new();
        runtime.apply_temperature_event(temperature_event(1, 30.0));
        let trigger_mitigation = runtime.apply_temperature_event(temperature_event(2, 36.0));

        assert!(runtime.broken_sensors.contains(&1));
        assert!(runtime.broken_sensors.contains(&2));
        assert!(runtime.sensor_temperatures.is_empty());
        assert_eq!(runtime.current_state, GuardianState::Critical);
        assert!(trigger_mitigation);

        runtime.apply_temperature_event(temperature_event(1, 31.0));
        assert!(runtime.sensor_temperatures.is_empty());
    }

    #[test]
    fn unaffected_sensor_remains_active_after_pairwise_disagreement() {
        let mut runtime = GuardianRuntime::new();
        runtime.apply_temperature_event(temperature_event(1, 30.0));
        runtime.apply_temperature_event(temperature_event(2, 33.0));
        runtime.apply_temperature_event(temperature_event(3, 36.0));

        assert!(runtime.broken_sensors.contains(&1));
        assert!(runtime.broken_sensors.contains(&3));
        assert!(!runtime.broken_sensors.contains(&2));
        assert_eq!(runtime.sensor_temperatures.get(&2), Some(&33.0));
        assert_eq!(runtime.temperature_celsius, 33.0);
    }

    #[test]
    fn guardian_is_critical_when_all_observed_sensors_fail() {
        let mut runtime = GuardianRuntime::new();
        assert_ne!(runtime.current_state, GuardianState::Critical);

        runtime.apply_temperature_event(CabinTemperatureEvent {
            temperature_celsius: 30.0,
            timestamp_ms: 0,
            sensor_status: SensorStatus::Ok,
            sensor_id: 1,
        });
        runtime.apply_temperature_event(CabinTemperatureEvent {
            temperature_celsius: 30.0,
            timestamp_ms: 0,
            sensor_status: SensorStatus::Ok,
            sensor_id: 2,
        });

        let first_failure_triggered = runtime.apply_temperature_event(CabinTemperatureEvent {
            temperature_celsius: 30.0,
            timestamp_ms: 0,
            sensor_status: SensorStatus::Failed,
            sensor_id: 1,
        });
        assert!(!first_failure_triggered);

        let final_failure_triggered = runtime.apply_temperature_event(CabinTemperatureEvent {
            temperature_celsius: 30.0,
            timestamp_ms: 0,
            sensor_status: SensorStatus::Failed,
            sensor_id: 2,
        });

        assert_eq!(runtime.current_state, GuardianState::Critical);
        assert!(runtime.all_temperature_sensors_broken());
        assert!(final_failure_triggered);

        let mitigation = build_mitigation_request(&runtime);
        assert_eq!(mitigation.window_percentage, 25);
        assert!(mitigation.alarm_enabled);
        assert_eq!(mitigation.reason, "TEMPERATURE_SENSOR_FAILURE_ALL");

        runtime.mark_mitigation_requested();
        runtime.mark_mitigation_success(&mitigation);
        assert!(!runtime.apply_hvac_state_event(HvacStateEvent {
            target_temperature_celsius: 18,
            air_conditioning_active: true,
            fan_speed_percent: 100,
            fault_active: false,
            timestamp_ms: 1,
        }));
    }
}
