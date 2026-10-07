/**
 * SPDX-License-Identifier: Apache-2.0 AND CC0-1.0
 *
 * AI Disclosure: This file was largely AI-generated. The AI-generated
 * portions are made available under CC0-1.0 and not subject to the
 * project's licence. The human contributor has reviewed and verified
 * that the code is correct.
 *
 * Assisted-by: Anthropic Claude (Sonnet 5)
 *
 * az3166_serial_bridge — MXChip AZ3166 (Eclipse ThreadX) UART to uProtocol gateway
 *
 * Reads newline-delimited JSON sensor frames from the AZ3166's onboard
 * LSM6DSL (accelerometer + die temperature) and HTS221 (humidity), sent
 * over the board's ST-Link virtual COM port (the same USB cable used for
 * flashing/debugging), and republishes them as uProtocol events on the
 * Zenoh transport.
 *
 * Publishes TWO events per reading:
 *
 *   1. `CabinTemperatureEvent` on the *existing* cabin-temperature topic
 *      (the same one `temperature_sim` already uses) — this makes the
 *      AZ3166 a true drop-in real sensor: Guardian's own
 *      CLEAR/WARNING/CRITICAL thresholds react to it with zero changes to
 *      guardian.rs. NOTE: the published value is the LSM6DSL's internal
 *      die temperature, not true ambient cabin air temperature - it is
 *      real, it changes, but it is a noisier/weaker signal than a proper
 *      ambient sensor. Do not run this bridge and `temperature-sim`
 *      at the same time - they publish to the same topic and will fight.
 *
 *   2. `Az3166ImuEvent` on a new, dashboard-only topic, carrying the full
 *      raw reading (acceleration + die temp + humidity + sequence/uptime)
 *      for inspection - Guardian never sees this one. humidity_pct comes
 *      from the HTS221, a genuine ambient-air sensor (unlike die_temp_c),
 *      but isn't wired into any Guardian decision yet - dashboard-only.
 *
 * Wire format (one JSON object per line, UART @ 115200 baud by default):
 *   {"seq":1042,"accel_mg":[12,-980,34],"die_temp_c":27.4,"humidity_pct":41.3,"uptime_ms":58213}
 *
 * Environment variables:
 *   AZ3166_SERIAL_PORT   Serial device/port (e.g. "COM4" on Windows,
 *                        "/dev/ttyACM0" on Linux). No default - required.
 *   AZ3166_BAUD_RATE     Baud rate                (default: 115200)
 *   ZENOH_CONNECT        Zenoh router endpoint     (default: tcp/zenohd:7447)
 *   RUST_LOG             Log level                 (default: info)
 *
 * Platform note: this binary needs direct access to a USB serial device,
 * which Docker Desktop on Windows/macOS cannot grant a container (same
 * WSL2/Hyper-V isolation issue already documented for the automotive
 * Ethernet adapter and the S32K148 CDA). Run it natively
 * (`cargo run --bin az3166_serial_bridge`) on Windows/macOS; on a real
 * Linux host (e.g. the Raspberry Pi), it can run inside Docker via a
 * `devices:` mapping - see docker-compose.yml's `az3166-serial-bridge`
 * service.
 */

use guardian_sil::{
    az3166_imu_uri, make_uri_provider, open_up_transport, publish_json_event,
    vss_cabin_temperature_uri, Az3166ImuEvent, CabinTemperatureEvent, SensorStatus,
};
use serde::Deserialize;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_serial::SerialPortBuilderExt;
use tracing::{debug, info, warn};

/// One line of JSON exactly as printf'd by the firmware.
#[derive(Debug, Deserialize)]
struct Az3166SensorFrame {
    seq: u32,
    accel_mg: [f32; 3],
    die_temp_c: f32,
    #[serde(default)]
    humidity_pct: f32,
    uptime_ms: u64,
}

fn parse_sensor_line(line: &str) -> Option<Az3166SensorFrame> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    match serde_json::from_str::<Az3166SensorFrame>(line) {
        Ok(frame) => Some(frame),
        Err(e) => {
            // A torn/partial line after a replug or UART noise is expected
            // occasionally - log and keep going, never crash the bridge
            // over a single bad line.
            debug!("ignoring unparseable line ({}): {:?}", e, line);
            None
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "az3166_serial_bridge=info,info".to_string()),
        )
        .init();

    let serial_port = std::env::var("AZ3166_SERIAL_PORT")
        .map_err(|_| "AZ3166_SERIAL_PORT must be set (e.g. COM4 on Windows, /dev/ttyACM0 on Linux)")?;
    let baud_rate: u32 = std::env::var("AZ3166_BAUD_RATE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(115200);

    info!("=== AZ3166 UART -> uProtocol bridge ===");
    info!("    Serial port       : {}", serial_port);
    info!("    Baud rate         : {}", baud_rate);

    let uri_provider = make_uri_provider("az3166-serial-bridge", 0x9206, 0x01);
    let transport = open_up_transport(uri_provider).await?;
    let temperature_sink = vss_cabin_temperature_uri();
    let imu_sink = az3166_imu_uri();

    info!(
        "    uProtocol sinks   : {} (CabinTemperature), {} (IMU)",
        guardian_sil::TOPIC_CABIN_TEMPERATURE,
        guardian_sil::TOPIC_AZ3166_IMU
    );

    loop {
        match run_bridge_loop(&serial_port, baud_rate, &transport, &temperature_sink, &imu_sink).await
        {
            Ok(()) => {
                // run_bridge_loop only returns on EOF (device unplugged).
                warn!("serial port closed (device unplugged?), retrying in 3s");
            }
            Err(e) => {
                warn!("serial connection failed: {} - retrying in 3s", e);
            }
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

async fn run_bridge_loop(
    serial_port: &str,
    baud_rate: u32,
    transport: &std::sync::Arc<dyn up_rust::UTransport>,
    temperature_sink: &up_rust::UUri,
    imu_sink: &up_rust::UUri,
) -> Result<(), Box<dyn std::error::Error>> {
    let port = tokio_serial::new(serial_port, baud_rate)
        .timeout(Duration::from_secs(10))
        .open_native_async()?;
    info!("Connected to {}", serial_port);

    let mut lines = BufReader::new(port).lines();

    while let Some(line) = lines.next_line().await? {
        let Some(frame) = parse_sensor_line(&line) else {
            continue;
        };

        let now = now_ms();

        let temperature_event = CabinTemperatureEvent {
            temperature_celsius: frame.die_temp_c,
            timestamp_ms: now,
            sensor_status: SensorStatus::Ok,
        };
        if let Err(e) =
            publish_json_event(transport.clone(), temperature_sink.clone(), &temperature_event).await
        {
            warn!("failed to publish CabinTemperatureEvent: {:?}", e);
        }

        let imu_event = Az3166ImuEvent {
            acceleration_mg: frame.accel_mg,
            die_temperature_celsius: frame.die_temp_c,
            humidity_pct: frame.humidity_pct,
            seq: frame.seq,
            board_uptime_ms: frame.uptime_ms,
            timestamp_ms: now,
        };
        if let Err(e) = publish_json_event(transport.clone(), imu_sink.clone(), &imu_event).await {
            warn!("failed to publish Az3166ImuEvent: {:?}", e);
        }

        info!(
            "seq={} die_temp={:.1}C humidity={:.1}% accel_mg={:?} uptime={}ms",
            frame.seq, frame.die_temp_c, frame.humidity_pct, frame.accel_mg, frame.uptime_ms
        );
    }

    Ok(())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
