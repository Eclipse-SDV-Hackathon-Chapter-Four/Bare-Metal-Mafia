/**
 * someip_window_bridge — uProtocol → SOME/IP window state bridge
 *
 * Listens for uProtocol window state events and converts them to SOME/IP
 * Notification packets, sending them to the ThreadX temperature sensor
 * so it can apply the thermal model and lower temperature when the window
 * is opened.
 *
 * SOME/IP wire format (Service 0x5678, Event 0x8002):
 *   Bytes  0-1   Service ID  0x5678
 *   Bytes  2-3   Event ID    0x8002
 *   Bytes  4-7   Length      (big-endian uint32) = 0x00000001
 *   Bytes  8-9   Client ID
 *   Bytes 10-11  Session ID
 *   Byte  12     Protocol Version = 0x01
 *   Byte  13     Interface Version = 0x01
 *   Byte  14     Message Type = 0x02 (NOTIFICATION)
 *   Byte  15     Return Code  = 0x00
 *   Byte  16     Window percentage (0-100)
 */

use guardian_sil::{
    make_uri_provider, open_up_transport, decode_json_payload, vss_window_state_uri,
    WindowStateEvent,
};
use std::sync::Arc;
use async_trait::async_trait;
use tracing::{info, warn};
use up_rust::{UListener, UMessage};

// ── SOME/IP constants ────────────────────────────────────────────────────────

const WINDOW_SOMEIP_PACKET_LEN: usize = 17;  // 16-byte header + 1-byte payload

const WINDOW_SERVICE_ID: u16 = 0x5678;
const WINDOW_EVENT_ID: u16 = 0x8002;
const WINDOW_CLIENT_ID: u16 = 0x0002;
const WINDOW_IFACE_VER: u8 = 0x01;

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Craft a SOME/IP Notification packet for window state.
fn make_window_packet(window_pct: u8) -> [u8; WINDOW_SOMEIP_PACKET_LEN] {
    let mut buf = [0u8; WINDOW_SOMEIP_PACKET_LEN];

    /* Service ID 0x5678 */
    buf[0] = 0x56;
    buf[1] = 0x78;
    /* Event ID 0x8002 */
    buf[2] = 0x80;
    buf[3] = 0x02;
    /* Length = 0x00000001 (1 byte payload) */
    buf[4] = 0x00;
    buf[5] = 0x00;
    buf[6] = 0x00;
    buf[7] = 0x01;
    /* Client ID 0x0002 */
    buf[8] = 0x00;
    buf[9] = 0x02;
    /* Session ID = 1 (static for simplicity) */
    buf[10] = 0x00;
    buf[11] = 0x01;
    /* Protocol version */
    buf[12] = 0x01;
    /* Interface version */
    buf[13] = WINDOW_IFACE_VER;
    /* Message type: NOTIFICATION */
    buf[14] = 0x02;
    /* Return code: E_OK */
    buf[15] = 0x00;
    /* Window percentage */
    buf[16] = window_pct;

    buf
}

// ── Window listener ──────────────────────────────────────────────────────────

struct WindowStateListener {
    udp_sock: std::net::UdpSocket,
    target_addr: String,
}

#[async_trait]
impl UListener for WindowStateListener {
    async fn on_receive(&self, message: UMessage) {
        match decode_json_payload::<WindowStateEvent>(&message) {
            Ok(evt) => {
                let window_pct = evt.window_percentage.min(100);
                let packet = make_window_packet(window_pct);

                info!("uProtocol → SOME/IP  window={}%", window_pct);

                if let Err(e) = self.udp_sock.send_to(&packet, &self.target_addr) {
                    warn!("UDP send to {} failed: {}", self.target_addr, e);
                }
            }
            Err(e) => {
                warn!("failed to decode window state event: {:?}", e);
            }
        }
    }
}

// ── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_env_filter(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "someip_window_bridge=debug,info".to_string()),
        )
        .init();

    let threadx_addr =
        std::env::var("THREADX_SENSOR_IP").unwrap_or_else(|_| "127.0.0.1".to_string());
    let threadx_port: u16 = std::env::var("THREADX_SENSOR_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(30502);

    info!("=== uProtocol → SOME/IP window bridge ===");
    info!(
        "    Destination: {}:{} (SOME/IP Service 0x5678 / Event 0x8002)",
        threadx_addr, threadx_port
    );

    // ── uProtocol transport ──────────────────────────────────────────────────
    let uri_provider = make_uri_provider("uprot-window-bridge", 0x9204, 0x01);
    let transport = open_up_transport(uri_provider).await?;
    let window_uri = vss_window_state_uri();

    info!(
        "    Source: {} (uProtocol)",
        guardian_sil::TOPIC_WINDOW_STATE
    );

    // ── UDP socket to ThreadX sensor ─────────────────────────────────────────
    let udp_sock = std::net::UdpSocket::bind("0.0.0.0:0")?;
    udp_sock.set_nonblocking(true)?;

    let target_addr = format!("{}:{}", threadx_addr, threadx_port);

    // Register listener for window state events
    let listener = Arc::new(WindowStateListener {
        udp_sock,
        target_addr,
    });

    transport
        .register_listener(&window_uri, None, listener)
        .await?;

    info!("Bridge ready — listening for window state events…");

    // Keep the bridge running
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
}
