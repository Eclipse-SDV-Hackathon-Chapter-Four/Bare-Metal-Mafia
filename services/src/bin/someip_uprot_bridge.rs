/**
 * someip_uprot_bridge — SOME/IP to Eclipse uProtocol gateway
 *
 * Receives raw SOME/IP Notification packets from the ThreadX temperature
 * sensor (either the Docker Linux-port service or the Renode STM32F407
 * emulation) and re-publishes them as uProtocol CabinTemperatureEvent
 * messages over the Zenoh transport.
 *
 * SOME/IP wire format (Service 0x1234, Event 0x8001):
 *   Bytes  0-1   Service ID  0x1234
 *   Bytes  2-3   Event ID    0x8001
 *   Bytes  4-7   Length      (big-endian uint32)
 *   Bytes  8-9   Client ID
 *   Bytes 10-11  Session ID
 *   Byte  12     Protocol Version = 0x01
 *   Byte  13     Interface Version
 *   Byte  14     Message Type = 0x02 (NOTIFICATION)
 *   Byte  15     Return Code  = 0x00
 *   Bytes 16-19  Temperature (big-endian IEEE 754 float, °C)
 *   Bytes 20-27  Timestamp   (big-endian uint64, milliseconds)
 *
 * Environment variables:
 *   SOMEIP_LISTEN_ADDR  UDP bind address (default: 0.0.0.0)
 *   SOMEIP_LISTEN_PORT  UDP bind port    (default: 30501)
 *   ZENOH_CONNECT       Zenoh router endpoint (default: tcp/zenohd:7447)
 *   RUST_LOG            Log level        (default: info)
 */

use guardian_sil::{
    make_uri_provider, open_up_transport, publish_json_event, vss_cabin_temperature_uri,
    CabinTemperatureEvent, SensorStatus,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::net::UdpSocket;
use tracing::{debug, error, info, warn};

// ── SOME/IP constants ────────────────────────────────────────────────────────

const SOMEIP_HEADER_LEN: usize = 16;
const SOMEIP_MIN_PACKET: usize = SOMEIP_HEADER_LEN + 12; // header + float + timestamp

const TEMP_SERVICE_ID: u16 = 0x1234;
const TEMP_EVENT_ID: u16 = 0x8001;
const SOMEIP_MSG_TYPE_NOTIFICATION: u8 = 0x02;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u64
}

/// Parse a SOME/IP Notification packet and extract (temperature_celsius, timestamp_ms).
/// Returns None if the packet is malformed or not the expected service/event.
fn parse_someip_temperature(buf: &[u8]) -> Option<(f32, u64)> {
    if buf.len() < SOMEIP_MIN_PACKET {
        debug!(
            "packet too short: {} < {} bytes",
            buf.len(),
            SOMEIP_MIN_PACKET
        );
        return None;
    }

    let service_id = u16::from_be_bytes([buf[0], buf[1]]);
    let event_id = u16::from_be_bytes([buf[2], buf[3]]);
    let msg_type = buf[14];

    if service_id != TEMP_SERVICE_ID {
        debug!("unexpected service ID: 0x{:04X}", service_id);
        return None;
    }
    if event_id != TEMP_EVENT_ID {
        debug!("unexpected event ID: 0x{:04X}", event_id);
        return None;
    }
    if msg_type != SOMEIP_MSG_TYPE_NOTIFICATION {
        debug!("unexpected message type: 0x{:02X}", msg_type);
        return None;
    }

    // Temperature: big-endian IEEE 754 float at offset 16
    let temp_bits = u32::from_be_bytes([buf[16], buf[17], buf[18], buf[19]]);
    let temperature = f32::from_bits(temp_bits);

    // Basic sanity check — reject clearly invalid readings
    if !temperature.is_finite() || temperature < -40.0 || temperature > 120.0 {
        warn!("received out-of-range temperature: {}°C — discarding", temperature);
        return None;
    }

    // Timestamp: big-endian uint64 at offset 20
    let timestamp_ms = u64::from_be_bytes([
        buf[20], buf[21], buf[22], buf[23], buf[24], buf[25], buf[26], buf[27],
    ]);

    Some((temperature, timestamp_ms))
}

// ── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_env_filter(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "someip_uprot_bridge=debug,info".to_string()),
        )
        .init();

    let listen_addr =
        std::env::var("SOMEIP_LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0".to_string());
    let listen_port: u16 = std::env::var("SOMEIP_LISTEN_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(30501);

    let bind_addr = format!("{}:{}", listen_addr, listen_port);

    info!("=== SOME/IP → uProtocol bridge ===");
    info!(
        "    SOME/IP listener  : {}  (Service 0x1234 / Event 0x8001)",
        bind_addr
    );

    // ── uProtocol transport ──────────────────────────────────────────────────
    let uri_provider = make_uri_provider("someip-uprot-bridge", 0x9203, 0x01);
    let transport = open_up_transport(uri_provider).await?;
    let temp_uri = vss_cabin_temperature_uri();

    info!(
        "    uProtocol sink    : {}",
        guardian_sil::TOPIC_CABIN_TEMPERATURE
    );

    // ── UDP listener ─────────────────────────────────────────────────────────
    let sock = UdpSocket::bind(&bind_addr).await?;
    info!("Bridge ready — waiting for SOME/IP packets…");

    let mut buf = vec![0u8; 1500];
    loop {
        match sock.recv_from(&mut buf).await {
            Ok((len, src)) => {
                debug!("recv {} bytes from {}", len, src);

                match parse_someip_temperature(&buf[..len]) {
                    Some((temperature, ts)) => {
                        // Use the packet timestamp if non-zero, otherwise local clock
                        let timestamp_ms = if ts > 0 { ts } else { now_ms() };

                        let event = CabinTemperatureEvent {
                            temperature_celsius: temperature,
                            timestamp_ms,
                            sensor_status: SensorStatus::Ok,
                        };

                        info!(
                            "SOME/IP → uProtocol  src={} temp={:.1}°C ts={}ms",
                            src, temperature, timestamp_ms
                        );

                        if let Err(e) =
                            publish_json_event(transport.clone(), temp_uri.clone(), &event).await
                        {
                            warn!("uProtocol publish failed: {:?}", e);
                        }
                    }
                    None => {
                        debug!(
                            "ignored packet from {} ({} bytes)",
                            src, len
                        );
                    }
                }
            }
            Err(e) => {
                error!("UDP recv error: {}", e);
                break;
            }
        }
    }

    Ok(())
}
