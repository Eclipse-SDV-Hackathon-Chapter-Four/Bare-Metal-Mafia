/**
 * s32k148_doip_bridge — DoIP/UDS to Eclipse uProtocol gateway
 *
 * Periodically reads the `WindowPosition` UDS data identifier (DID 0xCF20)
 * from the real NXP S32K148EVB-Q176 running the OpenBSW referenceApp over
 * DoIP (ISO 13400-2), and republishes the value as a uProtocol
 * `S32WindowPositionEvent` on the Zenoh transport so the Guardian dashboard
 * (and, later, the Guardian Loop itself) never has to know DoIP/UDS exists.
 *
 * This is a minimal, hand-rolled DoIP + UDS client (no external DoIP crate),
 * matching the hand-rolled SOME/IP framing style already used by
 * `someip_uprot_bridge.rs` for the ThreadX temperature sensor path.
 *
 * IMPORTANT — network placement:
 * This binary needs *direct* L2/L3 access to the automotive Ethernet
 * segment the S32K148 lives on (the UDP/TCP broadcast + routing activation
 * handshake does not survive NAT). Run it on whichever machine is
 * physically connected to the automotive Ethernet hardware (USB adapter ->
 * media converter -> TJA1101 -> S32K148) — it only needs normal LAN/WLAN
 * reachability to wherever `zenohd` is running (set via ZENOH_CONNECT), not
 * to be on the same machine as the rest of the Guardian stack.
 *
 * DoIP wire format used here (ISO 13400-2):
 *   Header (8 bytes): protocol version, inverse version, payload type (u16
 *   big-endian), payload length (u32 big-endian).
 *
 *   Routing Activation Request  (payload type 0x0005)
 *   Routing Activation Response (payload type 0x0006)
 *   Diagnostic Message          (payload type 0x8001), carries raw UDS bytes
 *
 * We deliberately do NOT send DiagnosticMessagePositiveAck (0x8002) back to
 * the ECU — the reference OpenSOVD CDA config for this exact ECU disables
 * that too (`send_diagnostic_message_ack = false` in opensovd-cda.toml),
 * because upstream OpenBSW's DoIP server does not expect/handle it cleanly.
 *
 * Environment variables:
 *   S32K148_IP            ECU IP address          (default: 192.168.0.200)
 *   S32K148_DOIP_PORT     DoIP TCP port            (default: 13400)
 *   S32K148_UDS_ADDRESS   ECU UDS logical address  (default: 0x002A, from
 *                         executables/referenceApp/configuration/include/app/appConfig.h)
 *   S32K148_TESTER_ADDRESS  Our own tester logical address (default: 0x0E00)
 *   S32K148_WINDOW_DID    DID to read              (default: 0xCF20)
 *   POLL_INTERVAL_S       Poll period in seconds   (default: 2)
 *   ZENOH_CONNECT         Zenoh router endpoint    (default: tcp/zenohd:7447)
 *   RUST_LOG              Log level                (default: info)
 */

use guardian_sil::{
    make_uri_provider, open_up_transport, publish_json_event, s32_window_position_uri,
    S32WindowPositionEvent,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tracing::{info, warn};

const DOIP_PROTOCOL_VERSION: u8 = 0x02;
const DOIP_INVERSE_PROTOCOL_VERSION: u8 = 0xFD;

const PAYLOAD_TYPE_ROUTING_ACTIVATION_REQUEST: u16 = 0x0005;
const PAYLOAD_TYPE_ROUTING_ACTIVATION_RESPONSE: u16 = 0x0006;
const PAYLOAD_TYPE_DIAGNOSTIC_MESSAGE: u16 = 0x8001;
const PAYLOAD_TYPE_DIAG_MESSAGE_POSITIVE_ACK: u16 = 0x8002;
const PAYLOAD_TYPE_DIAG_MESSAGE_NEGATIVE_ACK: u16 = 0x8003;

const ROUTING_ACTIVATION_TYPE_DEFAULT: u8 = 0x00;
const ROUTING_ACTIVATION_SUCCESS: u8 = 0x10;

const UDS_SID_READ_DATA_BY_IDENTIFIER: u8 = 0x22;
const UDS_SID_READ_DATA_BY_IDENTIFIER_POSITIVE_RESPONSE: u8 = 0x62;
const UDS_SID_NEGATIVE_RESPONSE: u8 = 0x7F;

const IO_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug)]
enum DoIpError {
    Io(std::io::Error),
    Protocol(String),
}

impl std::fmt::Display for DoIpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DoIpError::Io(e) => write!(f, "I/O error: {}", e),
            DoIpError::Protocol(msg) => write!(f, "protocol error: {}", msg),
        }
    }
}

impl From<std::io::Error> for DoIpError {
    fn from(e: std::io::Error) -> Self {
        DoIpError::Io(e)
    }
}

/// Build a DoIP frame: 8-byte header + payload.
fn build_doip_frame(payload_type: u16, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(8 + payload.len());
    frame.push(DOIP_PROTOCOL_VERSION);
    frame.push(DOIP_INVERSE_PROTOCOL_VERSION);
    frame.extend_from_slice(&payload_type.to_be_bytes());
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(payload);
    frame
}

/// Read exactly one DoIP frame (header + payload) from the stream.
async fn read_doip_frame(stream: &mut TcpStream) -> Result<(u16, Vec<u8>), DoIpError> {
    let mut header = [0u8; 8];
    timeout(IO_TIMEOUT, stream.read_exact(&mut header))
        .await
        .map_err(|_| DoIpError::Protocol("timeout reading DoIP header".into()))??;

    let payload_type = u16::from_be_bytes([header[2], header[3]]);
    let payload_len =
        u32::from_be_bytes([header[4], header[5], header[6], header[7]]) as usize;

    let mut payload = vec![0u8; payload_len];
    if payload_len > 0 {
        timeout(IO_TIMEOUT, stream.read_exact(&mut payload))
            .await
            .map_err(|_| DoIpError::Protocol("timeout reading DoIP payload".into()))??;
    }

    Ok((payload_type, payload))
}

/// Connect, perform routing activation, send ReadDataByIdentifier(did), and
/// return the raw data bytes from the positive response (without the
/// service-id/DID echo prefix).
async fn read_did_over_doip(
    ecu_addr: &str,
    uds_address: u16,
    tester_address: u16,
    did: u16,
) -> Result<Vec<u8>, DoIpError> {
    let mut stream = timeout(IO_TIMEOUT, TcpStream::connect(ecu_addr))
        .await
        .map_err(|_| DoIpError::Protocol(format!("timeout connecting to {}", ecu_addr)))??;

    // --- Routing Activation ---------------------------------------------
    let mut activation_payload = Vec::with_capacity(7);
    activation_payload.extend_from_slice(&tester_address.to_be_bytes());
    activation_payload.push(ROUTING_ACTIVATION_TYPE_DEFAULT);
    activation_payload.extend_from_slice(&[0u8; 4]); // reserved

    let request = build_doip_frame(PAYLOAD_TYPE_ROUTING_ACTIVATION_REQUEST, &activation_payload);
    stream.write_all(&request).await?;

    let (payload_type, payload) = read_doip_frame(&mut stream).await?;
    if payload_type != PAYLOAD_TYPE_ROUTING_ACTIVATION_RESPONSE {
        return Err(DoIpError::Protocol(format!(
            "expected routing activation response (0x0006), got 0x{:04X}",
            payload_type
        )));
    }
    if payload.len() < 5 || payload[4] != ROUTING_ACTIVATION_SUCCESS {
        return Err(DoIpError::Protocol(format!(
            "routing activation failed, response code: {:?}",
            payload.get(4)
        )));
    }

    // --- Diagnostic Message: ReadDataByIdentifier -----------------------
    let mut uds_request = Vec::with_capacity(3);
    uds_request.push(UDS_SID_READ_DATA_BY_IDENTIFIER);
    uds_request.extend_from_slice(&did.to_be_bytes());

    let mut diag_payload = Vec::with_capacity(4 + uds_request.len());
    diag_payload.extend_from_slice(&tester_address.to_be_bytes());
    diag_payload.extend_from_slice(&uds_address.to_be_bytes());
    diag_payload.extend_from_slice(&uds_request);

    let request = build_doip_frame(PAYLOAD_TYPE_DIAGNOSTIC_MESSAGE, &diag_payload);
    stream.write_all(&request).await?;

    // The ECU may send a positive/negative ACK (0x8002/0x8003) before the
    // actual diagnostic message with the UDS response - skip those, we only
    // care about the real 0x8001 diagnostic message carrying UDS data.
    for _ in 0..3 {
        let (payload_type, payload) = read_doip_frame(&mut stream).await?;
        match payload_type {
            PAYLOAD_TYPE_DIAG_MESSAGE_POSITIVE_ACK | PAYLOAD_TYPE_DIAG_MESSAGE_NEGATIVE_ACK => {
                continue;
            }
            PAYLOAD_TYPE_DIAGNOSTIC_MESSAGE => {
                if payload.len() < 5 {
                    return Err(DoIpError::Protocol("diagnostic message too short".into()));
                }
                let uds_response = &payload[4..];
                return parse_read_data_response(uds_response, did);
            }
            other => {
                return Err(DoIpError::Protocol(format!(
                    "unexpected DoIP payload type 0x{:04X}",
                    other
                )));
            }
        }
    }

    Err(DoIpError::Protocol(
        "no diagnostic message received after ACK(s)".into(),
    ))
}

fn parse_read_data_response(uds_response: &[u8], expected_did: u16) -> Result<Vec<u8>, DoIpError> {
    if uds_response.is_empty() {
        return Err(DoIpError::Protocol("empty UDS response".into()));
    }

    if uds_response[0] == UDS_SID_NEGATIVE_RESPONSE {
        let nrc = uds_response.get(2).copied().unwrap_or(0xFF);
        return Err(DoIpError::Protocol(format!(
            "ECU returned negative response, NRC=0x{:02X}",
            nrc
        )));
    }

    if uds_response[0] != UDS_SID_READ_DATA_BY_IDENTIFIER_POSITIVE_RESPONSE {
        return Err(DoIpError::Protocol(format!(
            "unexpected UDS response SID 0x{:02X}",
            uds_response[0]
        )));
    }

    if uds_response.len() < 3 {
        return Err(DoIpError::Protocol("UDS response missing DID echo".into()));
    }

    let echoed_did = u16::from_be_bytes([uds_response[1], uds_response[2]]);
    if echoed_did != expected_did {
        return Err(DoIpError::Protocol(format!(
            "DID mismatch: expected 0x{:04X}, got 0x{:04X}",
            expected_did, echoed_did
        )));
    }

    Ok(uds_response[3..].to_vec())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "s32k148_doip_bridge=info,info".to_string()),
        )
        .init();

    let ecu_ip = std::env::var("S32K148_IP").unwrap_or_else(|_| "192.168.0.200".to_string());
    let doip_port: u16 = std::env::var("S32K148_DOIP_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(13400);
    let uds_address: u16 = std::env::var("S32K148_UDS_ADDRESS")
        .ok()
        .and_then(|v| u16::from_str_radix(v.trim_start_matches("0x"), 16).ok())
        .unwrap_or(0x002A);
    let tester_address: u16 = std::env::var("S32K148_TESTER_ADDRESS")
        .ok()
        .and_then(|v| u16::from_str_radix(v.trim_start_matches("0x"), 16).ok())
        .unwrap_or(0x0E00);
    let window_did: u16 = std::env::var("S32K148_WINDOW_DID")
        .ok()
        .and_then(|v| u16::from_str_radix(v.trim_start_matches("0x"), 16).ok())
        .unwrap_or(0xCF20);
    let poll_interval_s: u64 = std::env::var("POLL_INTERVAL_S")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2);

    let ecu_addr = format!("{}:{}", ecu_ip, doip_port);

    info!("=== DoIP/UDS -> uProtocol bridge (S32K148 WindowPosition) ===");
    info!("    ECU (DoIP)        : {}", ecu_addr);
    info!("    UDS target addr   : 0x{:04X}", uds_address);
    info!("    Tester addr       : 0x{:04X}", tester_address);
    info!("    DID               : 0x{:04X}", window_did);
    info!("    Poll interval     : {}s", poll_interval_s);

    let uri_provider = make_uri_provider("s32k148-doip-bridge", 0x9205, 0x01);
    let transport = open_up_transport(uri_provider).await?;
    let sink = s32_window_position_uri();

    info!(
        "    uProtocol sink    : {}",
        guardian_sil::TOPIC_S32_WINDOW_POSITION
    );

    loop {
        match read_did_over_doip(&ecu_addr, uds_address, tester_address, window_did).await {
            Ok(data) => {
                let percentage = data.first().copied().unwrap_or(0);
                let event = S32WindowPositionEvent {
                    percentage,
                    source: "s32k148-doip".to_string(),
                    timestamp_ms: now_ms(),
                };

                info!("DoIP read OK: WindowPosition = {}%", percentage);

                if let Err(e) = publish_json_event(transport.clone(), sink.clone(), &event).await {
                    warn!("uProtocol publish failed: {:?}", e);
                }
            }
            Err(e) => {
                warn!("DoIP read failed: {} (ECU unreachable or not responding)", e);
            }
        }

        tokio::time::sleep(Duration::from_secs(poll_interval_s)).await;
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
