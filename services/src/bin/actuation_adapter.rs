use std::pin::Pin;
use std::sync::Arc;

use guardian_sil::{
    diag_alarm_cmd_uri, diag_hvac_cmd_uri, diag_window_cmd_uri, make_uri_provider,
    mitigation_rpc_uri, open_up_transport, publish_json_event, AlarmCommand, HvacCommand,
    MitigationRequest, MitigationResponse, WindowPositionCommand, RID_MITIGATION_REQUEST_RPC,
};
use tracing::{info, warn};
use up_rust::communication::{InMemoryRpcServer, RequestHandler, RpcServer, ServiceInvocationError, UPayload};
use up_rust::UAttributes;

struct MitigationHandler {
    transport: Arc<dyn up_rust::UTransport>,
}

impl RequestHandler for MitigationHandler {
    fn handle_request<'life0, 'life1, 'async_trait>(
        &'life0 self,
        _resource_id: u16,
        _message_attributes: &'life1 UAttributes,
        request_payload: Option<UPayload>,
    ) -> Pin<
        Box<
            dyn std::future::Future<Output = Result<Option<UPayload>, ServiceInvocationError>>
                + Send
                + 'async_trait,
        >,
    >
    where
        Self: 'async_trait,
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        Box::pin(async move {
            let request_payload = request_payload
                .ok_or_else(|| ServiceInvocationError::InvalidArgument("missing payload".to_string()))?;

            let data = request_payload.payload();
            let request: MitigationRequest = serde_json::from_slice(&data)
                .map_err(|e| ServiceInvocationError::InvalidArgument(e.to_string()))?;

            info!(
                "Mitigation request id={} reason={} ts={} window={} fan={} alarm={}",
                request.request_id,
                request.reason,
                request.timestamp_ms,
                request.window_percentage,
                request.fan_enabled,
                request.alarm_enabled
            );

            let window_cmd = WindowPositionCommand {
                request_id: request.request_id.clone(),
                percentage: request.window_percentage,
            };

            let alarm_cmd = AlarmCommand {
                request_id: request.request_id.clone(),
                enabled: request.alarm_enabled,
            };

            let hvac_cmd = HvacCommand {
                request_id: request.request_id.clone(),
                target_temperature_celsius: request.hvac_target_temperature_celsius,
                air_conditioning_active: request.hvac_power_enabled,
                fan_speed_percent: request.hvac_fan_speed_percent.min(100),
            };

            let mut success = true;

            success &= publish_json_event(self.transport.clone(), diag_hvac_cmd_uri(), &hvac_cmd)
                .await
                .is_ok();
            success &= publish_json_event(self.transport.clone(), diag_window_cmd_uri(), &window_cmd)
                .await
                .is_ok();
            success &= publish_json_event(self.transport.clone(), diag_alarm_cmd_uri(), &alarm_cmd)
                .await
                .is_ok();

            let response = MitigationResponse {
                request_id: request.request_id,
                success,
                details: "diagnostic commands published".to_string(),
            };

            let response_payload = serde_json::to_vec(&response)
                .map_err(|e| ServiceInvocationError::Internal(e.to_string()))?;

            Ok(Some(UPayload::new(
                response_payload,
                up_rust::UPayloadFormat::UPAYLOAD_FORMAT_JSON,
            )))
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_env_filter(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "actuation_adapter=info,info".to_string()),
        )
        .init();

    let uri_provider = make_uri_provider("guardian-actuation", 0x9100, 0x01);
    let transport = open_up_transport(uri_provider.clone()).await?;

    let rpc_server = InMemoryRpcServer::new(transport.clone(), uri_provider);
    rpc_server
        .register_endpoint(
            None,
            RID_MITIGATION_REQUEST_RPC,
            Arc::new(MitigationHandler {
                transport: transport.clone(),
            }),
        )
        .await?;

    info!("Actuation adapter RPC endpoint online at {}", mitigation_rpc_uri());

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
        if false {
            warn!("noop");
        }
    }
}
