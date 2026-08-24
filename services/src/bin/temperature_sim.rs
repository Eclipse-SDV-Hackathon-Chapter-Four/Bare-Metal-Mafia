use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use guardian_sil::{
    decode_json_payload, make_uri_provider, open_up_transport, publish_json_event,
    vss_cabin_temperature_uri, vss_window_state_uri, CabinTemperatureEvent, SensorStatus,
    WindowStateEvent,
};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn};
use up_rust::{UListener, UMessage};

struct WindowStateListener {
    window_state: Arc<Mutex<WindowStateEvent>>,
}

#[async_trait]
impl UListener for WindowStateListener {
    async fn on_receive(&self, message: UMessage) {
        match decode_json_payload::<WindowStateEvent>(&message) {
            Ok(evt) => {
                let mut guard = self.window_state.lock().await;
                *guard = evt;
            }
            Err(err) => warn!("failed to decode window state event: {}", err),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "temperature_sim=info,info".to_string()),
        )
        .init();

    let heat_gain_per_step = env_f32("HEAT_GAIN_PER_STEP", 0.18);
    let alarm_heat_per_step = env_f32("ALARM_HEAT_PER_STEP", 0.02);
    let cooling_per_step_full_window = env_f32("COOLING_PER_STEP_FULL_WINDOW", 1.20);
    let publish_interval_s = env_u64("PUBLISH_INTERVAL_S", 5);
    let uri_provider = make_uri_provider("temperature-sim", 0x9202, 0x01);
    let transport = open_up_transport(uri_provider).await?;
    let window_state = Arc::new(Mutex::new(WindowStateEvent {
        window_percentage: 0,
        alarm_enabled: false,
        timestamp_ms: now_ms(),
    }));

    transport
        .register_listener(
            &vss_window_state_uri(),
            None,
            Arc::new(WindowStateListener {
                window_state: window_state.clone(),
            }),
        )
        .await?;

    tokio::time::sleep(Duration::from_secs(1)).await;

    let stages = vec![26.0_f32, 36.0_f32, 43.0_f32];
    for temperature in stages {
        let event = CabinTemperatureEvent {
            temperature_celsius: temperature,
            timestamp_ms: now_ms(),
            sensor_status: SensorStatus::Ok,
        };

        publish_with_retry(transport.clone(), &event).await?;
        info!("published temperature: {:.1}C", temperature);
        tokio::time::sleep(Duration::from_secs(4)).await;
    }

    let mut temperature_celsius = 43.0_f32;
    let ambient_celsius = 24.0_f32;

    loop {
        let snapshot = window_state.lock().await.clone();
        let window_pct = snapshot.window_percentage.min(100);
        let alarm_enabled = snapshot.alarm_enabled;

        // Minimal closed-loop model:
        // - cabin naturally heats up under sun load
        // - opened window adds cooling proportional to opening percentage
        // - alarm adds slight extra heat to keep this deterministic but simple
        let heat_gain = heat_gain_per_step;
        let alarm_heat = if alarm_enabled {
            alarm_heat_per_step
        } else {
            0.0_f32
        };
        let cooling = (window_pct as f32 / 100.0) * cooling_per_step_full_window;
        let delta = heat_gain + alarm_heat - cooling;

        temperature_celsius = (temperature_celsius + delta).clamp(ambient_celsius, 48.0);

        let event = CabinTemperatureEvent {
            temperature_celsius,
            timestamp_ms: now_ms(),
            sensor_status: SensorStatus::Ok,
        };

        publish_with_retry(transport.clone(), &event).await?;
        info!(
            "closed-loop temperature: {:.1}C (window={}%, alarm={}, delta={:+.2})",
            temperature_celsius, window_pct, alarm_enabled, delta
        );

        tokio::time::sleep(Duration::from_secs(publish_interval_s)).await;
    }
}

fn env_f32(name: &str, default: f32) -> f32 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(default)
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(default)
}

async fn publish_with_retry(
    transport: Arc<dyn up_rust::UTransport>,
    payload: &CabinTemperatureEvent,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut attempt = 0u8;

    while attempt < 10 {
        let result = publish_json_event(transport.clone(), vss_cabin_temperature_uri(), payload).await;

        match result {
            Ok(_) => return Ok(()),
            Err(err) => {
                warn!("publish failed: {}", err);
            }
        }

        attempt += 1;
        tokio::time::sleep(Duration::from_millis(750)).await;
    }

    Err("failed to publish after retries".into())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
