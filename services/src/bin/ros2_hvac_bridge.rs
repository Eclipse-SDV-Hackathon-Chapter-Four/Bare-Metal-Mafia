use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};
use axum::{Json, Router};
use guardian_sil::{
    decode_json_payload, hvac_state_uri, make_uri_provider, open_up_transport, publish_json_event,
    uds_hvac_cmd_uri, vss_hvac_active_state_uri, vss_hvac_set_temperature_uri,
    HvacActiveStateEvent, HvacCommand, HvacSetTemperatureEvent, HvacStateEvent,
};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tracing::{info, warn};
use up_rust::{UListener, UMessage, UTransport};

#[derive(Debug, Clone, Serialize)]
struct HvacControllerSnapshot {
    target_temperature_celsius: i8,
    requested_air_conditioning_active: bool,
    effective_air_conditioning_active: bool,
    fan_speed_percent: u8,
    fault_active: bool,
    last_request_id: Option<String>,
    timestamp_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HvacControllerState {
    target_temperature_celsius: i8,
    requested_air_conditioning_active: bool,
    fan_speed_percent: u8,
    fault_active: bool,
    last_request_id: Option<String>,
}

impl Default for HvacControllerState {
    fn default() -> Self {
        Self {
            target_temperature_celsius: 22,
            requested_air_conditioning_active: false,
            fan_speed_percent: 0,
            fault_active: false,
            last_request_id: None,
        }
    }
}

impl HvacControllerState {
    fn effective_air_conditioning_active(&self) -> bool {
        self.requested_air_conditioning_active && !self.fault_active
    }

    fn snapshot(&self) -> HvacControllerSnapshot {
        HvacControllerSnapshot {
            target_temperature_celsius: self.target_temperature_celsius,
            requested_air_conditioning_active: self.requested_air_conditioning_active,
            effective_air_conditioning_active: self.effective_air_conditioning_active(),
            fan_speed_percent: self.fan_speed_percent,
            fault_active: self.fault_active,
            last_request_id: self.last_request_id.clone(),
            timestamp_ms: now_ms(),
        }
    }
}

#[derive(Clone)]
struct AppState {
    hvac_state: Arc<Mutex<HvacControllerState>>,
    transport: Arc<dyn UTransport>,
}

#[derive(Debug, Deserialize)]
struct FaultToggleRequest {
    fault_active: bool,
}

struct HvacCommandListener {
    app: AppState,
}

#[async_trait]
impl UListener for HvacCommandListener {
    async fn on_receive(&self, message: UMessage) {
        match decode_json_payload::<HvacCommand>(&message) {
            Ok(cmd) => {
                let snapshot = {
                    let mut guard = self.app.hvac_state.lock().await;
                    guard.target_temperature_celsius = cmd.target_temperature_celsius;
                    guard.requested_air_conditioning_active = cmd.air_conditioning_active;
                    guard.fan_speed_percent = cmd.fan_speed_percent.min(100);
                    guard.last_request_id = Some(cmd.request_id.clone());
                    guard.snapshot()
                };

                if let Err(err) = publish_hvac_state(&self.app.transport, &snapshot).await {
                    warn!("failed to publish HVAC state after command: {}", err);
                } else {
                    info!(
                        "HVAC command applied request_id={} target={}C active={} fan={} fault={}",
                        cmd.request_id,
                        snapshot.target_temperature_celsius,
                        snapshot.effective_air_conditioning_active,
                        snapshot.fan_speed_percent,
                        snapshot.fault_active
                    );
                }
            }
            Err(err) => warn!("failed to decode HVAC command: {}", err),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_env_filter(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "ros2_hvac_bridge=info,reqwest=warn,info".to_string()),
        )
        .init();

    let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port = std::env::var("PORT").unwrap_or_else(|_| "8093".to_string());
    let addr = format!("{}:{}", host, port);
    let uri_provider = make_uri_provider("ros2-hvac-bridge", 0x9501, 0x01);
    let transport = open_up_transport(uri_provider).await?;
    let app_state = AppState {
        hvac_state: Arc::new(Mutex::new(HvacControllerState::default())),
        transport: transport.clone(),
    };

    transport
        .register_listener(
            &uds_hvac_cmd_uri(),
            None,
            Arc::new(HvacCommandListener {
                app: app_state.clone(),
            }),
        )
        .await?;

    let initial_snapshot = app_state.hvac_state.lock().await.snapshot();
    publish_hvac_state(&transport, &initial_snapshot).await?;

    let app = Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .route("/api/state", get(get_state))
        .route("/api/fault", post(set_fault))
        .with_state(app_state);

    let listener = TcpListener::bind(&addr).await?;
    info!("ROS2 HVAC bridge UI listening on {}", addr);
    info!("ros2_medkit diagnostics remain available on port 18080; configuration sync is disabled because the gateway has no writable /configurations backend for hvac_simulator");

    axum::serve(listener, app).await?;
    Ok(())
}

async fn index() -> impl IntoResponse {
    Html(INDEX_HTML)
}

async fn health() -> StatusCode {
    StatusCode::OK
}

async fn get_state(State(app): State<AppState>) -> Json<HvacControllerSnapshot> {
    let guard = app.hvac_state.lock().await;
    Json(guard.snapshot())
}

async fn set_fault(
    State(app): State<AppState>,
    Json(payload): Json<FaultToggleRequest>,
) -> Result<Json<HvacControllerSnapshot>, StatusCode> {
    let snapshot = {
        let mut guard = app.hvac_state.lock().await;
        guard.fault_active = payload.fault_active;
        guard.snapshot()
    };

    publish_hvac_state(&app.transport, &snapshot)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(snapshot))
}

async fn publish_hvac_state(
    transport: &Arc<dyn UTransport>,
    snapshot: &HvacControllerSnapshot,
) -> Result<(), up_rust::UStatus> {
    publish_json_event(
        transport.clone(),
        vss_hvac_set_temperature_uri(),
        &HvacSetTemperatureEvent {
            temperature_celsius: snapshot.target_temperature_celsius,
            timestamp_ms: snapshot.timestamp_ms,
        },
    )
    .await?;

    publish_json_event(
        transport.clone(),
        vss_hvac_active_state_uri(),
        &HvacActiveStateEvent {
            active: snapshot.effective_air_conditioning_active,
            timestamp_ms: snapshot.timestamp_ms,
        },
    )
    .await?;

    publish_json_event(
        transport.clone(),
        hvac_state_uri(),
        &HvacStateEvent {
            target_temperature_celsius: snapshot.target_temperature_celsius,
            air_conditioning_active: snapshot.effective_air_conditioning_active,
            fan_speed_percent: snapshot.fan_speed_percent,
            fault_active: snapshot.fault_active,
            timestamp_ms: snapshot.timestamp_ms,
        },
    )
    .await
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

const INDEX_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Guardian HVAC Fault Console</title>
  <style>
    :root {
      --bg: #f4efe6;
      --panel: rgba(255, 252, 246, 0.88);
      --ink: #1b241f;
      --muted: #5f6b63;
      --accent: #0e8c61;
      --danger: #cc4b37;
      --border: rgba(27, 36, 31, 0.12);
    }
    * { box-sizing: border-box; }
    body {
      margin: 0;
      min-height: 100vh;
      font-family: Georgia, "Times New Roman", serif;
      color: var(--ink);
      background:
        radial-gradient(circle at top left, rgba(14, 140, 97, 0.14), transparent 30%),
        radial-gradient(circle at bottom right, rgba(204, 75, 55, 0.15), transparent 28%),
        linear-gradient(135deg, #efe6d7 0%, #f7f4ed 48%, #e2ebdf 100%);
      display: grid;
      place-items: center;
      padding: 24px;
    }
    .panel {
      width: min(860px, 100%);
      background: var(--panel);
      border: 1px solid var(--border);
      border-radius: 28px;
      box-shadow: 0 24px 80px rgba(30, 40, 34, 0.12);
      overflow: hidden;
      backdrop-filter: blur(18px);
    }
    .hero {
      padding: 28px 30px 18px;
      border-bottom: 1px solid var(--border);
      background: linear-gradient(120deg, rgba(14, 140, 97, 0.08), rgba(255,255,255,0));
    }
    h1 {
      margin: 0 0 8px;
      font-size: clamp(2rem, 4vw, 3.1rem);
      line-height: 0.95;
      letter-spacing: -0.04em;
    }
    .subtitle {
      margin: 0;
      color: var(--muted);
      max-width: 56ch;
      font-size: 1rem;
    }
    .grid {
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(190px, 1fr));
      gap: 14px;
      padding: 22px 24px 12px;
    }
    .card {
      border: 1px solid var(--border);
      border-radius: 20px;
      padding: 16px 18px;
      background: rgba(255,255,255,0.55);
    }
    .label {
      font-size: 0.8rem;
      text-transform: uppercase;
      letter-spacing: 0.1em;
      color: var(--muted);
      margin-bottom: 10px;
    }
    .value {
      font-size: 2rem;
      line-height: 1;
    }
    .value.small {
      font-size: 1.15rem;
    }
    .actions {
      padding: 12px 24px 28px;
      display: flex;
      flex-wrap: wrap;
      gap: 12px;
      align-items: center;
    }
    button {
      border: 0;
      border-radius: 999px;
      padding: 14px 18px;
      font: inherit;
      font-weight: 700;
      cursor: pointer;
      transition: transform 140ms ease, opacity 140ms ease;
    }
    button:hover { transform: translateY(-1px); }
    .ok { background: var(--accent); color: white; }
    .fault { background: var(--danger); color: white; }
    .status-pill {
      padding: 10px 14px;
      border-radius: 999px;
      background: rgba(27, 36, 31, 0.06);
      color: var(--muted);
    }
    .status-pill strong { color: var(--ink); }
  </style>
</head>
<body>
  <main class="panel">
    <section class="hero">
      <h1>Guardian HVAC</h1>
      <p class="subtitle">ROS2 HVAC controller state over Zenoh and up-rust. Use this panel to inject an HVAC fault and watch the guardian escalate from setpoint control to window mitigation.</p>
    </section>
    <section class="grid">
      <article class="card"><div class="label">Target Temperature</div><div class="value" id="target">--</div></article>
      <article class="card"><div class="label">Effective AC State</div><div class="value small" id="active">--</div></article>
      <article class="card"><div class="label">Fan Speed</div><div class="value" id="fan">--</div></article>
      <article class="card"><div class="label">Fault State</div><div class="value small" id="fault">--</div></article>
    </section>
    <section class="actions">
      <button class="fault" id="injectFault">Inject HVAC Fault</button>
      <button class="ok" id="clearFault">Clear Fault</button>
      <div class="status-pill">Last request: <strong id="requestId">none</strong></div>
    </section>
  </main>
  <script>
    async function fetchState() {
      const response = await fetch('/api/state');
      const data = await response.json();
      document.getElementById('target').textContent = `${data.target_temperature_celsius}°C`;
      document.getElementById('active').textContent = data.effective_air_conditioning_active ? 'ACTIVE' : 'INACTIVE';
      document.getElementById('fan').textContent = `${data.fan_speed_percent}%`;
      document.getElementById('fault').textContent = data.fault_active ? 'FAULTED' : 'HEALTHY';
      document.getElementById('requestId').textContent = data.last_request_id || 'none';
    }

    async function setFault(faultActive) {
      await fetch('/api/fault', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ fault_active: faultActive })
      });
      await fetchState();
    }

    document.getElementById('injectFault').addEventListener('click', () => setFault(true));
    document.getElementById('clearFault').addEventListener('click', () => setFault(false));
    fetchState();
    setInterval(fetchState, 1500);
  </script>
</body>
</html>
"#;
