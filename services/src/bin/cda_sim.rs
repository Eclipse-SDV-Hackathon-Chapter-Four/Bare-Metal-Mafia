use async_trait::async_trait;
use guardian_sil::{
    decode_json_payload, diag_alarm_cmd_uri, diag_hvac_cmd_uri, diag_window_cmd_uri,
    make_uri_provider, open_up_transport, publish_json_event, uds_alarm_cmd_uri,
    uds_hvac_cmd_uri, uds_window_cmd_uri, AlarmCommand, HvacCommand, WindowPositionCommand,
};
use tracing::{info, warn};
use up_rust::{UListener, UMessage, UTransport};

struct DiagWindowListener {
    transport: std::sync::Arc<dyn UTransport>,
}

#[async_trait]
impl UListener for DiagWindowListener {
    async fn on_receive(&self, message: UMessage) {
        match decode_json_payload::<WindowPositionCommand>(&message) {
            Ok(cmd) => {
                if publish_json_event(self.transport.clone(), uds_window_cmd_uri(), &cmd)
                    .await
                    .is_ok()
                {
                    info!(
                        "CDA->UDS window position {}% request_id={} forwarded",
                        cmd.percentage, cmd.request_id
                    );
                }
            }
            Err(err) => warn!("Invalid diag window payload: {}", err),
        }
    }
}

struct DiagAlarmListener {
    transport: std::sync::Arc<dyn UTransport>,
}

#[async_trait]
impl UListener for DiagAlarmListener {
    async fn on_receive(&self, message: UMessage) {
        match decode_json_payload::<AlarmCommand>(&message) {
            Ok(cmd) => {
                if publish_json_event(self.transport.clone(), uds_alarm_cmd_uri(), &cmd)
                    .await
                    .is_ok()
                {
                    info!(
                        "CDA->UDS alarm enabled={} request_id={} forwarded",
                        cmd.enabled, cmd.request_id
                    );
                }
            }
            Err(err) => warn!("Invalid diag alarm payload: {}", err),
        }
    }
}

struct DiagHvacListener {
    transport: std::sync::Arc<dyn UTransport>,
}

#[async_trait]
impl UListener for DiagHvacListener {
    async fn on_receive(&self, message: UMessage) {
        match decode_json_payload::<HvacCommand>(&message) {
            Ok(cmd) => {
                if publish_json_event(self.transport.clone(), uds_hvac_cmd_uri(), &cmd)
                    .await
                    .is_ok()
                {
                    info!(
                        "CDA->UDS HVAC target={}C ac={} fan={} request_id={} forwarded",
                        cmd.target_temperature_celsius,
                        cmd.air_conditioning_active,
                        cmd.fan_speed_percent,
                        cmd.request_id
                    );
                }
            }
            Err(err) => warn!("Invalid diag HVAC payload: {}", err),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "cda_sim=info,info".to_string()))
        .init();

    let uri_provider = make_uri_provider("cda-sim", 0x9301, 0x01);
    let transport = open_up_transport(uri_provider).await?;

    info!(
        "CDA sim forwarding diagnostics {:?}|{:?} -> {:?}|{:?}",
        diag_window_cmd_uri(),
        diag_alarm_cmd_uri(),
        uds_window_cmd_uri(),
        uds_alarm_cmd_uri()
    );

    transport
        .register_listener(
            &diag_window_cmd_uri(),
            None,
            std::sync::Arc::new(DiagWindowListener {
                transport: transport.clone(),
            }),
        )
        .await?;

    transport
        .register_listener(
            &diag_alarm_cmd_uri(),
            None,
            std::sync::Arc::new(DiagAlarmListener {
                transport: transport.clone(),
            }),
        )
        .await?;

    transport
        .register_listener(
            &diag_hvac_cmd_uri(),
            None,
            std::sync::Arc::new(DiagHvacListener {
                transport: transport.clone(),
            }),
        )
        .await?;

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
    }
}
