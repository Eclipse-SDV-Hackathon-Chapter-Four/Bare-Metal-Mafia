use serde::{Deserialize, Serialize};
use std::sync::Arc;
use up_rust::{
    communication::UPayload, LocalUriProvider, StaticUriProvider, UMessage, UMessageBuilder,
    UPayloadFormat, UStatus, UTransport, UUri,
};
use up_transport_zenoh::UPTransportZenoh;

pub const TOPIC_CHILD_PRESENCE: &str =
    "up/sdv/guardian/vss/Vehicle.Cabin.Seat.Row2.ChildPresence";
pub const TOPIC_CABIN_TEMPERATURE: &str =
    "up/sdv/guardian/vss/Vehicle.Cabin.HVAC.AmbientAirTemperature";
pub const TOPIC_GUARDIAN_STATE: &str = "up/sdv/guardian/vss/Vehicle.Cabin.Guardian.State";
pub const TOPIC_MITIGATION_REQUEST: &str = "up/sdv/guardian/rpc/mitigation/request";
pub const TOPIC_MITIGATION_RESPONSE: &str = "up/sdv/guardian/rpc/mitigation/response";
pub const TOPIC_DIAG_WINDOW_CMD: &str = "up/sdv/guardian/diag/window/cmd";
pub const TOPIC_DIAG_ALARM_CMD: &str = "up/sdv/guardian/diag/alarm/cmd";
pub const TOPIC_DIAG_HVAC_CMD: &str = "up/sdv/guardian/diag/hvac/cmd";
pub const TOPIC_UDS_WINDOW_CMD: &str = "up/sdv/guardian/uds/window/cmd";
pub const TOPIC_UDS_ALARM_CMD: &str = "up/sdv/guardian/uds/alarm/cmd";
pub const TOPIC_UDS_HVAC_CMD: &str = "up/sdv/guardian/uds/hvac/cmd";
pub const TOPIC_WINDOW_STATE: &str = "up/sdv/guardian/vss/Vehicle.Cabin.Window.Row2.Left.State";
pub const TOPIC_HVAC_SET_TEMPERATURE: &str =
    "up/sdv/guardian/vss/Vehicle.Cabin.HVAC.Station.Row1.Left.Temperature";
pub const TOPIC_HVAC_ACTIVE_STATE: &str =
    "up/sdv/guardian/vss/Vehicle.Cabin.HVAC.IsAirConditioningActive";
pub const TOPIC_HVAC_STATE: &str = "up/sdv/guardian/hvac/state";
pub const TOPIC_S32_WINDOW_POSITION: &str = "up/sdv/guardian/s32k148/window_position";
// AZ3166 additions below.
// Assisted-by: Anthropic Claude (Sonnet 5)
pub const TOPIC_AZ3166_IMU: &str = "up/sdv/guardian/az3166/imu";

pub const RID_CHILD_PRESENCE_EVENT: u16 = 0x9001;
pub const RID_CABIN_TEMPERATURE_EVENT: u16 = 0x9002;
pub const RID_GUARDIAN_STATE_EVENT: u16 = 0x9003;
pub const RID_WINDOW_STATE_EVENT: u16 = 0x9004;
pub const RID_HVAC_SET_TEMPERATURE_EVENT: u16 = 0x9005;
pub const RID_HVAC_ACTIVE_STATE_EVENT: u16 = 0x9006;
pub const RID_HVAC_STATE_EVENT: u16 = 0x9007;
pub const RID_S32_WINDOW_POSITION_EVENT: u16 = 0x9008;
pub const RID_AZ3166_IMU_EVENT: u16 = 0x9009;

pub const RID_DIAG_WINDOW_CMD_EVENT: u16 = 0x9010;
pub const RID_DIAG_ALARM_CMD_EVENT: u16 = 0x9011;
pub const RID_DIAG_HVAC_CMD_EVENT: u16 = 0x9012;
pub const RID_UDS_WINDOW_CMD_EVENT: u16 = 0x9020;
pub const RID_UDS_ALARM_CMD_EVENT: u16 = 0x9021;
pub const RID_UDS_HVAC_CMD_EVENT: u16 = 0x9022;

pub const RID_MITIGATION_REQUEST_RPC: u16 = 0x1001;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChildPresenceEvent {
    pub present: bool,
    pub confidence: f32,
    pub zone: Option<String>,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CabinTemperatureEvent {
    pub temperature_celsius: f32,
    pub timestamp_ms: u64,
    pub sensor_status: SensorStatus,
    pub sensor_id: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SensorStatus {
    Ok,
    Degraded,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GuardianState {
    Clear,
    Monitoring,
    Warning,
    Critical,
    Mitigating,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GuardianSnapshot {
    pub state: GuardianState,
    pub child_present: bool,
    pub temperature_celsius: f32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MitigationRequest {
    pub request_id: String,
    pub hvac_target_temperature_celsius: i8,
    pub hvac_fan_speed_percent: u8,
    pub hvac_power_enabled: bool,
    pub window_percentage: u8,
    pub fan_enabled: bool,
    pub alarm_enabled: bool,
    pub reason: String,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MitigationResponse {
    pub request_id: String,
    pub success: bool,
    pub details: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WindowPositionCommand {
    pub request_id: String,
    pub percentage: u8,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AlarmCommand {
    pub request_id: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HvacCommand {
    pub request_id: String,
    pub target_temperature_celsius: i8,
    pub air_conditioning_active: bool,
    pub fan_speed_percent: u8,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HvacSetTemperatureEvent {
    pub temperature_celsius: i8,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HvacActiveStateEvent {
    pub active: bool,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WindowStateEvent {
    pub window_percentage: u8,
    pub alarm_enabled: bool,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HvacStateEvent {
    pub target_temperature_celsius: i8,
    pub air_conditioning_active: bool,
    pub fan_speed_percent: u8,
    pub fault_active: bool,
    pub timestamp_ms: u64,
}

/// WindowPosition (DID 0xCF20) read from the real S32K148 OpenBSW ECU over
/// DoIP/UDS. Separate from `WindowStateEvent` (which is published by the
/// simulated `window_controller_sim`) so the dashboard can show both the
/// simulated actuator and the real hardware side by side without either one
/// overwriting the other.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct S32WindowPositionEvent {
    pub percentage: u8,
    pub source: String,
    pub timestamp_ms: u64,
}

/// LSM6DSL reading from the real MXChip AZ3166 (Eclipse ThreadX), dashboard-only
/// detail. Note: `die_temperature_celsius` is the accelerometer chip's own die
/// temperature, not true ambient cabin air temperature - it is published
/// separately here for inspection. The Guardian-relevant value is republished
/// by the same bridge onto the *existing* `CabinTemperatureEvent`/
/// `vss_cabin_temperature_uri()` topic (same one `temperature_sim` uses), so
/// Guardian's real decision logic reacts to it without any `guardian.rs` change.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Az3166ImuEvent {
    pub acceleration_mg: [f32; 3],
    pub die_temperature_celsius: f32,
    pub seq: u32,
    pub board_uptime_ms: u64,
    pub timestamp_ms: u64,
}

pub fn vss_child_presence_uri() -> UUri {
    UUri::try_from_parts("guardian-vss", 0x9000, 0x01, RID_CHILD_PRESENCE_EVENT).unwrap()
}

pub fn vss_cabin_temperature_uri() -> UUri {
    UUri::try_from_parts("guardian-vss", 0x9000, 0x01, RID_CABIN_TEMPERATURE_EVENT).unwrap()
}

pub fn vss_guardian_state_uri() -> UUri {
    UUri::try_from_parts("guardian-vss", 0x9000, 0x01, RID_GUARDIAN_STATE_EVENT).unwrap()
}

pub fn vss_window_state_uri() -> UUri {
    UUri::try_from_parts("guardian-vss", 0x9000, 0x01, RID_WINDOW_STATE_EVENT).unwrap()
}

pub fn vss_hvac_set_temperature_uri() -> UUri {
    UUri::try_from_parts("guardian-vss", 0x9000, 0x01, RID_HVAC_SET_TEMPERATURE_EVENT).unwrap()
}

pub fn vss_hvac_active_state_uri() -> UUri {
    UUri::try_from_parts("guardian-vss", 0x9000, 0x01, RID_HVAC_ACTIVE_STATE_EVENT).unwrap()
}

pub fn hvac_state_uri() -> UUri {
    UUri::try_from_parts("guardian-hvac", 0x9005, 0x01, RID_HVAC_STATE_EVENT).unwrap()
}

pub fn s32_window_position_uri() -> UUri {
    UUri::try_from_parts("guardian-vss", 0x9000, 0x01, RID_S32_WINDOW_POSITION_EVENT).unwrap()
}

pub fn az3166_imu_uri() -> UUri {
    UUri::try_from_parts("guardian-vss", 0x9000, 0x01, RID_AZ3166_IMU_EVENT).unwrap()
}

pub fn diag_window_cmd_uri() -> UUri {
    UUri::try_from_parts("guardian-diag", 0x9010, 0x01, RID_DIAG_WINDOW_CMD_EVENT).unwrap()
}

pub fn diag_alarm_cmd_uri() -> UUri {
    UUri::try_from_parts("guardian-diag", 0x9010, 0x01, RID_DIAG_ALARM_CMD_EVENT).unwrap()
}

pub fn diag_hvac_cmd_uri() -> UUri {
    UUri::try_from_parts("guardian-diag", 0x9010, 0x01, RID_DIAG_HVAC_CMD_EVENT).unwrap()
}

pub fn uds_window_cmd_uri() -> UUri {
    UUri::try_from_parts("guardian-uds", 0x9020, 0x01, RID_UDS_WINDOW_CMD_EVENT).unwrap()
}

pub fn uds_alarm_cmd_uri() -> UUri {
    UUri::try_from_parts("guardian-uds", 0x9020, 0x01, RID_UDS_ALARM_CMD_EVENT).unwrap()
}

pub fn uds_hvac_cmd_uri() -> UUri {
    UUri::try_from_parts("guardian-uds", 0x9020, 0x01, RID_UDS_HVAC_CMD_EVENT).unwrap()
}

pub fn mitigation_rpc_uri() -> UUri {
    UUri::try_from_parts("guardian-actuation", 0x9100, 0x01, RID_MITIGATION_REQUEST_RPC).unwrap()
}

pub fn make_uri_provider(
    authority: &str,
    entity_id: u32,
    major_version: u8,
) -> Arc<dyn LocalUriProvider> {
    Arc::new(StaticUriProvider::new(authority, entity_id, major_version))
}

pub async fn open_up_transport(
    uri_provider: Arc<dyn LocalUriProvider>,
) -> Result<Arc<dyn UTransport>, Box<dyn std::error::Error>> {
    UPTransportZenoh::try_init_log_from_env();

    let mut config = zenoh::Config::default();
    if let Ok(endpoint) = std::env::var("ZENOH_CONNECT") {
        let payload = format!("[\"{}\"]", endpoint);
        let _ = config.insert_json5("connect/endpoints", &payload);
    }

    let transport = UPTransportZenoh::builder(uri_provider.get_authority())
        .expect("invalid authority name")
        .with_config(config)
        .build()
        .await
        .map(Arc::new)?;

    Ok(transport)
}

pub async fn publish_json_event<T: serde::Serialize>(
    transport: Arc<dyn UTransport>,
    sink: UUri,
    value: &T,
) -> Result<(), UStatus> {
    let data = serde_json::to_vec(value)
        .map_err(|e| UStatus::fail_with_code(up_rust::UCode::INVALID_ARGUMENT, e.to_string()))?;
    let payload = UPayload::new(data, UPayloadFormat::UPAYLOAD_FORMAT_JSON);
    let payload_format = payload.payload_format();
    let mut builder = UMessageBuilder::publish(sink);
    let message = builder
        .build_with_payload(payload.payload(), payload_format)
        .map_err(|e| UStatus::fail_with_code(up_rust::UCode::INVALID_ARGUMENT, e.to_string()))?;

    transport.send(message).await
}

pub fn decode_json_payload<T: serde::de::DeserializeOwned>(
    message: &UMessage,
) -> Result<T, Box<dyn std::error::Error + Send + Sync>> {
    let Some(payload) = message.payload.clone() else {
        return Err("missing payload".into());
    };
    Ok(serde_json::from_slice::<T>(&payload)?)
}

pub async fn open_zenoh_session() -> Result<zenoh::Session, zenoh::Error> {
    let mut config = zenoh::Config::default();

    if let Ok(endpoint) = std::env::var("ZENOH_CONNECT") {
        let payload = format!("[\"{}\"]", endpoint);
        let _ = config.insert_json5("connect/endpoints", &payload);
    }

    zenoh::open(config).await
}

pub fn evaluate_state(child_present: bool, temperature_celsius: f32) -> GuardianState {
    if !child_present {
        return GuardianState::Clear;
    }

    if temperature_celsius >= 28.5 {
        GuardianState::Critical
    } else if temperature_celsius >= 25.0 {
        GuardianState::Warning
    } else {
        GuardianState::Monitoring
    }
}
