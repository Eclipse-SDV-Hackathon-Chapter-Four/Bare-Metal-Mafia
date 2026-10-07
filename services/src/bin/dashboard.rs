use std::sync::Arc;

use async_trait::async_trait;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};
use axum::{Json, Router};
use guardian_sil::{
    az3166_imu_uri, decode_json_payload, hvac_state_uri, make_uri_provider, open_up_transport,
    publish_json_event, s32_window_position_uri, uds_window_cmd_uri, vss_cabin_temperature_uri,
    vss_child_presence_uri, vss_guardian_state_uri, vss_window_state_uri, Az3166ImuEvent,
    CabinTemperatureEvent, ChildPresenceEvent, GuardianSnapshot, HvacStateEvent,
    S32WindowPositionEvent, WindowPositionCommand, WindowStateEvent,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tracing::{info, warn};
use up_rust::{UListener, UMessage, UTransport};

#[derive(Debug, Clone, Serialize)]
struct DashboardSnapshot {
    guardian: Option<GuardianSnapshot>,
    child_presence: Option<ChildPresenceEvent>,
    cabin_temperature: Option<CabinTemperatureEvent>,
    window_state: Option<WindowStateEvent>,
    hvac_state: Option<HvacStateEvent>,
    s32_window_position: Option<S32WindowPositionEvent>,
    az3166_imu: Option<Az3166ImuEvent>,
}

impl DashboardSnapshot {
    fn new() -> Self {
        Self {
            guardian: None,
            child_presence: None,
            cabin_temperature: None,
            window_state: None,
            hvac_state: None,
            s32_window_position: None,
            az3166_imu: None,
        }
    }
}

#[derive(Clone)]
struct AppState {
    snapshot: Arc<Mutex<DashboardSnapshot>>,
    medkit_faults_url: String,
    hvac_fault_control_url: String,
    http: reqwest::Client,
    transport: Arc<dyn UTransport>,
}

#[derive(Debug, Deserialize)]
struct FaultToggleRequest {
    fault_active: bool,
}

#[derive(Debug, Deserialize)]
struct ChildPresenceToggleRequest {
    present: bool,
}

#[derive(Debug, Deserialize)]
struct WindowCommandRequest {
    percentage: u8,
}

struct ChildPresenceListener {
    snapshot: Arc<Mutex<DashboardSnapshot>>,
}

struct TemperatureListener {
    snapshot: Arc<Mutex<DashboardSnapshot>>,
}

struct GuardianListener {
    snapshot: Arc<Mutex<DashboardSnapshot>>,
}

struct WindowStateListener {
    snapshot: Arc<Mutex<DashboardSnapshot>>,
}

struct HvacStateListener {
    snapshot: Arc<Mutex<DashboardSnapshot>>,
}

struct S32WindowPositionListener {
    snapshot: Arc<Mutex<DashboardSnapshot>>,
}

// AZ3166 additions below.
// Assisted-by: Anthropic Claude (Sonnet 5)
struct Az3166ImuListener {
    snapshot: Arc<Mutex<DashboardSnapshot>>,
}

#[async_trait]
impl UListener for ChildPresenceListener {
    async fn on_receive(&self, message: UMessage) {
        match decode_json_payload::<ChildPresenceEvent>(&message) {
            Ok(event) => {
                self.snapshot.lock().await.child_presence = Some(event);
            }
            Err(err) => warn!("dashboard failed to decode child presence: {}", err),
        }
    }
}

#[async_trait]
impl UListener for TemperatureListener {
    async fn on_receive(&self, message: UMessage) {
        match decode_json_payload::<CabinTemperatureEvent>(&message) {
            Ok(event) => {
                self.snapshot.lock().await.cabin_temperature = Some(event);
            }
            Err(err) => warn!("dashboard failed to decode temperature: {}", err),
        }
    }
}

#[async_trait]
impl UListener for GuardianListener {
    async fn on_receive(&self, message: UMessage) {
        match decode_json_payload::<GuardianSnapshot>(&message) {
            Ok(event) => {
                self.snapshot.lock().await.guardian = Some(event);
            }
            Err(err) => warn!("dashboard failed to decode guardian snapshot: {}", err),
        }
    }
}

#[async_trait]
impl UListener for WindowStateListener {
    async fn on_receive(&self, message: UMessage) {
        match decode_json_payload::<WindowStateEvent>(&message) {
            Ok(event) => {
                self.snapshot.lock().await.window_state = Some(event);
            }
            Err(err) => warn!("dashboard failed to decode window state: {}", err),
        }
    }
}

#[async_trait]
impl UListener for HvacStateListener {
    async fn on_receive(&self, message: UMessage) {
        match decode_json_payload::<HvacStateEvent>(&message) {
            Ok(event) => {
                self.snapshot.lock().await.hvac_state = Some(event);
            }
            Err(err) => warn!("dashboard failed to decode HVAC state: {}", err),
        }
    }
}

#[async_trait]
impl UListener for S32WindowPositionListener {
    async fn on_receive(&self, message: UMessage) {
        match decode_json_payload::<S32WindowPositionEvent>(&message) {
            Ok(event) => {
                self.snapshot.lock().await.s32_window_position = Some(event);
            }
            Err(err) => warn!("dashboard failed to decode S32K148 window position: {}", err),
        }
    }
}

#[async_trait]
impl UListener for Az3166ImuListener {
    async fn on_receive(&self, message: UMessage) {
        match decode_json_payload::<Az3166ImuEvent>(&message) {
            Ok(event) => {
                self.snapshot.lock().await.az3166_imu = Some(event);
            }
            Err(err) => warn!("dashboard failed to decode AZ3166 IMU event: {}", err),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "dashboard=info,info".to_string()))
        .init();

    let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port = std::env::var("PORT").unwrap_or_else(|_| "8094".to_string());
    let addr = format!("{}:{}", host, port);
    let snapshot = Arc::new(Mutex::new(DashboardSnapshot::new()));

    let uri_provider = make_uri_provider("guardian-dashboard", 0x9601, 0x01);
    let transport = open_up_transport(uri_provider).await?;

    let app_state = AppState {
        snapshot: snapshot.clone(),
        medkit_faults_url: std::env::var("MEDKIT_FAULTS_URL")
            .unwrap_or_else(|_| "http://ros2-hvac:8080/api/v1/faults".to_string()),
        hvac_fault_control_url: std::env::var("HVAC_FAULT_CONTROL_URL")
            .unwrap_or_else(|_| "http://ros2-hvac:8093/api/fault".to_string()),
        http: reqwest::Client::builder().build()?,
        transport: transport.clone(),
    };

    transport
        .register_listener(
            &vss_child_presence_uri(),
            None,
            Arc::new(ChildPresenceListener {
                snapshot: snapshot.clone(),
            }),
        )
        .await?;

    transport
        .register_listener(
            &vss_cabin_temperature_uri(),
            None,
            Arc::new(TemperatureListener {
                snapshot: snapshot.clone(),
            }),
        )
        .await?;

    transport
        .register_listener(
            &vss_guardian_state_uri(),
            None,
            Arc::new(GuardianListener {
                snapshot: snapshot.clone(),
            }),
        )
        .await?;

    transport
        .register_listener(
            &vss_window_state_uri(),
            None,
            Arc::new(WindowStateListener {
                snapshot: snapshot.clone(),
            }),
        )
        .await?;

    transport
        .register_listener(
            &hvac_state_uri(),
            None,
            Arc::new(HvacStateListener {
                snapshot: snapshot.clone(),
            }),
        )
        .await?;

    transport
        .register_listener(
            &s32_window_position_uri(),
            None,
            Arc::new(S32WindowPositionListener {
                snapshot: snapshot.clone(),
            }),
        )
        .await?;

    transport
        .register_listener(
            &az3166_imu_uri(),
            None,
            Arc::new(Az3166ImuListener { snapshot }),
        )
        .await?;

    let app = Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .route("/api/state", get(get_state))
        .route("/api/faults", get(get_faults))
        .route("/api/hvac/fault", post(set_hvac_fault))
        .route("/api/child-presence", post(set_child_presence))
        .route("/api/window", post(set_window_position))
        .with_state(app_state);

    let listener = TcpListener::bind(&addr).await?;
    info!("Guardian dashboard listening on {}", addr);
    axum::serve(listener, app).await?;
    Ok(())
}

async fn index() -> impl IntoResponse {
    Html(INDEX_HTML)
}

async fn health() -> StatusCode {
    StatusCode::OK
}

async fn get_state(State(app): State<AppState>) -> Json<DashboardSnapshot> {
    Json(app.snapshot.lock().await.clone())
}

async fn get_faults(State(app): State<AppState>) -> Json<Value> {
    let response = match app.http.get(&app.medkit_faults_url).send().await {
        Ok(response) => response,
        Err(err) => {
            return Json(json!({
                "ok": false,
                "source": app.medkit_faults_url,
                "error": format!("send failed: {}", err),
            }));
        }
    };

    let response = match response.error_for_status() {
        Ok(response) => response,
        Err(err) => {
            return Json(json!({
                "ok": false,
                "source": app.medkit_faults_url,
                "error": format!("http error: {}", err),
            }));
        }
    };

    match response.json::<Value>().await {
        Ok(body) => Json(json!({
            "ok": true,
            "source": app.medkit_faults_url,
            "data": body,
        })),
        Err(err) => Json(json!({
            "ok": false,
            "source": app.medkit_faults_url,
            "error": format!("json decode failed: {}", err),
        })),
    }
}

async fn set_hvac_fault(
    State(app): State<AppState>,
    Json(payload): Json<FaultToggleRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let response = app
        .http
        .post(&app.hvac_fault_control_url)
        .json(&json!({ "fault_active": payload.fault_active }))
        .send()
        .await
        .map_err(|err| (StatusCode::BAD_GATEWAY, err.to_string()))?;

    let response = response
        .error_for_status()
        .map_err(|err| (StatusCode::BAD_GATEWAY, err.to_string()))?;

    let body = response
        .json::<Value>()
        .await
        .map_err(|err| (StatusCode::BAD_GATEWAY, err.to_string()))?;

    Ok(Json(body))
}

/// Publishes a ChildPresenceEvent over uProtocol - a real publish, not an
/// HTTP proxy like set_hvac_fault above, since there's no external HTTP
/// service for child presence (child-presence-sim publishes the same
/// topic directly). When this binary runs on the Raspberry Pi host
/// (ZENOH_CONNECT pointed at the AutoSD VM's zenohd via its hostfwd
/// port), this click crosses from the Pi straight into the VM's Guardian
/// over uProtocol, same pattern as az3166_serial_bridge.
async fn set_child_presence(
    State(app): State<AppState>,
    Json(payload): Json<ChildPresenceToggleRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let event = ChildPresenceEvent {
        present: payload.present,
        confidence: 1.0,
        zone: Some("dashboard-injected".to_string()),
        timestamp_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
    };

    publish_json_event(app.transport.clone(), vss_child_presence_uri(), &event)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;

    Ok(Json(json!({ "present": payload.present })))
}

/// Manual window override: publishes a WindowPositionCommand directly onto
/// `uds_window_cmd_uri()` - the exact topic `s32k148_doip_bridge.rs` (real
/// hardware) or `window_controller_sim.rs` (simulated) listens on. This
/// bypasses Guardian/actuation_adapter/cda_sim entirely on purpose: Guardian
/// currently never re-closes the window on its own once it escalates (it
/// only resets its own internal staging flags, see guardian.rs's
/// recompute_and_log), so this is the deliberate manual reset path. It also
/// doubles as an "open window on demand" button for demos/debugging
/// without needing to drive the full CRITICAL state through Guardian.
async fn set_window_position(
    State(app): State<AppState>,
    Json(payload): Json<WindowCommandRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let command = WindowPositionCommand {
        request_id: uuid::Uuid::new_v4().to_string(),
        percentage: payload.percentage,
    };

    publish_json_event(app.transport.clone(), uds_window_cmd_uri(), &command)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;

    Ok(Json(json!({ "percentage": payload.percentage })))
}

const INDEX_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Guardian Loop Dashboard</title>
  <style>
    :root {
      --bg: #f0e7db;
      --panel: rgba(255, 251, 245, 0.92);
      --line: rgba(23, 31, 28, 0.12);
      --ink: #171f1c;
      --muted: #61716b;
      --green: #158a61;
      --amber: #b88522;
      --red: #c4483b;
      --blue: #1f5ea8;
    }
    * { box-sizing: border-box; }
    body {
      margin: 0;
      color: var(--ink);
      font-family: "Segoe UI", "Helvetica Neue", sans-serif;
      background:
        radial-gradient(circle at top left, rgba(21,138,97,0.10), transparent 28%),
        radial-gradient(circle at bottom right, rgba(31,94,168,0.12), transparent 28%),
        linear-gradient(135deg, #f5ebdc 0%, #f9f7f2 45%, #dde9e7 100%);
      min-height: 100vh;
      padding: 28px;
    }
    .shell {
      max-width: 1200px;
      margin: 0 auto;
      display: grid;
      gap: 18px;
    }
    .hero, .grid > section, .faults {
      background: var(--panel);
      border: 1px solid var(--line);
      border-radius: 24px;
      box-shadow: 0 18px 60px rgba(25, 33, 31, 0.10);
      backdrop-filter: blur(16px);
    }
    .hero {
      padding: 26px 28px;
      display: flex;
      justify-content: space-between;
      gap: 16px;
      align-items: end;
    }
    h1 {
      margin: 0 0 8px;
      font-size: clamp(2rem, 4vw, 3.3rem);
      line-height: 0.94;
      letter-spacing: -0.05em;
    }
    .sub {
      color: var(--muted);
      max-width: 60ch;
      margin: 0;
    }
    .pills {
      display: flex;
      flex-wrap: wrap;
      gap: 10px;
    }
    .pill {
      padding: 10px 14px;
      border-radius: 999px;
      background: rgba(23, 31, 28, 0.06);
      color: var(--muted);
      font-size: 0.92rem;
    }
    .grid {
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
      gap: 18px;
    }
    .grid > section {
      padding: 18px;
    }
    .eyebrow {
      font-size: 0.78rem;
      text-transform: uppercase;
      letter-spacing: 0.12em;
      color: var(--muted);
      margin-bottom: 12px;
    }
    .value {
      font-size: 2rem;
      line-height: 1;
      letter-spacing: -0.04em;
      margin-bottom: 10px;
    }
    .meta {
      color: var(--muted);
      font-size: 0.95rem;
      line-height: 1.45;
    }
    .badge {
      display: inline-block;
      padding: 6px 10px;
      border-radius: 999px;
      font-size: 0.82rem;
      font-weight: 700;
      margin-top: 12px;
      background: rgba(23, 31, 28, 0.08);
    }
    .ok { color: var(--green); }
    .warn { color: var(--amber); }
    .bad { color: var(--red); }
    .info { color: var(--blue); }
    .actions {
      display: flex;
      gap: 12px;
      margin-top: 16px;
      flex-wrap: wrap;
    }
    button {
      border: 0;
      border-radius: 999px;
      padding: 12px 16px;
      font: inherit;
      font-weight: 700;
      cursor: pointer;
      color: white;
    }
    .fault-on { background: var(--red); }
    .fault-off { background: var(--green); }
    .faults {
      padding: 18px;
    }
    pre {
      margin: 0;
      padding: 16px;
      border-radius: 18px;
      background: #121917;
      color: #d8eee0;
      overflow: auto;
      font-size: 0.86rem;
    }
  </style>
</head>
<body>
  <main class="shell">
    <section class="hero">
      <div>
        <h1>Guardian Loop Dashboard</h1>
        <p class="sub">Live overview of child presence, cabin heat, Guardian mitigation state, window actuation, HVAC state, and medkit fault data across the simulated stack.</p>
      </div>
      <div class="pills">
        <div class="pill">Zenoh/uProtocol events</div>
        <div class="pill">ROS 2 HVAC + medkit faults</div>
        <div class="pill">HVAC fault control proxy</div>
      </div>
    </section>

    <section class="grid">
      <section>
        <div class="eyebrow">Guardian</div>
        <div class="value" id="guardianState">--</div>
        <div class="meta" id="guardianMeta">Waiting for Guardian state event.</div>
      </section>
      <section>
        <div class="eyebrow">Cabin Temperature</div>
        <div class="value" id="temperature">--</div>
        <div class="meta" id="temperatureMeta">Waiting for temperature event.</div>
      </section>
      <section>
        <div class="eyebrow">Child Presence</div>
        <div class="value" id="childPresence">--</div>
        <div class="meta" id="childMeta">Waiting for child presence event.</div>
        <div class="actions">
          <button class="fault-on" onclick="toggleChildPresence(true)">Set Child Present</button>
          <button class="fault-off" onclick="toggleChildPresence(false)">Set Child Absent</button>
        </div>
      </section>
      <section>
        <div class="eyebrow">Window</div>
        <div class="value" id="windowValue">--</div>
        <div class="meta" id="windowMeta">Waiting for window state event.</div>
        <div class="actions">
          <button class="fault-on" onclick="setWindow(25)">Open Window</button>
          <button class="fault-off" onclick="setWindow(0)">Reset Window (Close)</button>
        </div>
      </section>
      <section>
        <div class="eyebrow">HVAC</div>
        <div class="value" id="hvacValue">--</div>
        <div class="meta" id="hvacMeta">Waiting for HVAC state event.</div>
        <div class="actions">
          <button class="fault-on" onclick="toggleFault(true)">Inject HVAC Fault</button>
          <button class="fault-off" onclick="toggleFault(false)">Clear HVAC Fault</button>
        </div>
      </section>
      <section>
        <div class="eyebrow">S32K148 Window Position <span class="badge info">Real HW</span></div>
        <div class="value" id="s32WindowValue">--</div>
        <div class="meta" id="s32WindowMeta">Waiting for DoIP bridge event.</div>
      </section>
      <section>
        <div class="eyebrow">AZ3166 IMU <span class="badge info">Real HW</span></div>
        <div class="value" id="az3166Value">--</div>
        <div class="meta" id="az3166Humidity">--</div>
        <div class="meta" id="az3166Meta">Waiting for AZ3166 serial bridge event.</div>
      </section>
      <section>
        <div class="eyebrow">Observation Links</div>
        <div class="meta">
          <div>Guardian HTTP: <code>localhost:8080/state</code></div>
          <div>Window HTTP: <code>localhost:8092/state</code></div>
          <div>Medkit faults: <code>localhost:18080/api/v1/faults</code></div>
          <div>HVAC fault UI: <code>localhost:18081</code></div>
        </div>
      </section>
    </section>

    <section class="faults">
      <div class="eyebrow">Medkit Faults</div>
      <pre id="faultsJson">Loading...</pre>
    </section>
  </main>

  <script>
    async function refreshState() {
      try {
        const response = await fetch('/api/state');
        const state = await response.json();

        const guardian = state.guardian;
        document.getElementById('guardianState').textContent = guardian ? guardian.state : '--';
        document.getElementById('guardianMeta').textContent = guardian
          ? `Child present: ${guardian.child_present} | Temperature: ${guardian.temperature_celsius.toFixed(1)}°C`
          : 'Waiting for Guardian state event.';

        const temp = state.cabin_temperature;
        document.getElementById('temperature').textContent = temp ? `${temp.temperature_celsius.toFixed(1)}°C` : '--';
        document.getElementById('temperatureMeta').textContent = temp
          ? `Sensor status: ${temp.sensor_status} | ts=${temp.timestamp_ms}`
          : 'Waiting for temperature event.';

        const child = state.child_presence;
        document.getElementById('childPresence').textContent = child ? (child.present ? 'Detected' : 'Clear') : '--';
        document.getElementById('childMeta').textContent = child
          ? `Confidence ${child.confidence} | Zone ${child.zone || 'unknown'}`
          : 'Waiting for child presence event.';

        const windowState = state.window_state;
        document.getElementById('windowValue').textContent = windowState ? `${windowState.window_percentage}%` : '--';
        document.getElementById('windowMeta').textContent = windowState
          ? `Alarm enabled: ${windowState.alarm_enabled} | ts=${windowState.timestamp_ms}`
          : 'No value yet - this card listens for window_controller_sim (the simulated actuator), which isn\'t running in the real-hardware stack. See "S32K148 Window Position (Real HW)" below for the actual actuator state. The buttons here always work regardless, since they command the real/simulated actuator directly.';

        const hvac = state.hvac_state;
        document.getElementById('hvacValue').textContent = hvac
          ? `${hvac.target_temperature_celsius}°C / ${hvac.fan_speed_percent}%`
          : '--';
        document.getElementById('hvacMeta').textContent = hvac
          ? `Active: ${hvac.air_conditioning_active} | Fault: ${hvac.fault_active} | ts=${hvac.timestamp_ms}`
          : 'Waiting for HVAC state event.';

        const s32Window = state.s32_window_position;
        document.getElementById('s32WindowValue').textContent = s32Window ? `${s32Window.percentage}%` : '--';
        document.getElementById('s32WindowMeta').textContent = s32Window
          ? `Source: ${s32Window.source} (DID 0xCF20 over DoIP) | ts=${s32Window.timestamp_ms}`
          : 'Waiting for DoIP bridge event.';

        const az3166 = state.az3166_imu;
        document.getElementById('az3166Value').textContent = az3166 ? `${az3166.die_temperature_celsius.toFixed(1)}°C (die)` : '--';
        document.getElementById('az3166Humidity').textContent = az3166
          ? `Humidity (HTS221): ${az3166.humidity_pct.toFixed(1)}%`
          : '';
        document.getElementById('az3166Meta').textContent = az3166
          ? `Accel mg: [${az3166.acceleration_mg.map(v => v.toFixed(0)).join(', ')}] | seq=${az3166.seq} | uptime=${az3166.board_uptime_ms}ms`
          : 'Waiting for AZ3166 serial bridge event.';
      } catch (err) {
        console.error(err);
      }
    }

    async function refreshFaults() {
      try {
        const response = await fetch('/api/faults');
        const body = await response.json();
        document.getElementById('faultsJson').textContent = JSON.stringify(body, null, 2);
      } catch (err) {
        document.getElementById('faultsJson').textContent = `Faults unavailable: ${err}`;
      }
    }

    async function toggleFault(faultActive) {
      await fetch('/api/hvac/fault', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ fault_active: faultActive })
      });
      await refreshState();
      await refreshFaults();
    }

    async function toggleChildPresence(present) {
      await fetch('/api/child-presence', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ present })
      });
      await refreshState();
    }

    async function setWindow(percentage) {
      await fetch('/api/window', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ percentage })
      });
      await refreshState();
    }

    refreshState();
    refreshFaults();
    setInterval(refreshState, 1500);
    setInterval(refreshFaults, 4000);
  </script>
</body>
</html>
"#;
