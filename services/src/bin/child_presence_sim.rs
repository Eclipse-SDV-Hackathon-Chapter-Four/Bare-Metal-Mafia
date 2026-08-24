use std::time::{Duration, SystemTime, UNIX_EPOCH};

use guardian_sil::{
    make_uri_provider, open_up_transport, publish_json_event, vss_child_presence_uri,
    ChildPresenceEvent,
};
use tracing::{info, warn};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "child_presence_sim=info,info".to_string()),
        )
        .init();

    let uri_provider = make_uri_provider("child-presence-sim", 0x9201, 0x01);
    let transport = open_up_transport(uri_provider).await?;

    let plan = vec![(false, 0u64), (true, 4u64)];

    for (present, wait_s) in plan {
        if wait_s > 0 {
            tokio::time::sleep(Duration::from_secs(wait_s)).await;
        }

        let event = ChildPresenceEvent {
            present,
            confidence: if present { 0.98 } else { 0.99 },
            zone: Some("rear_center".to_string()),
            timestamp_ms: now_ms(),
        };

        publish_with_retry(transport.clone(), &event).await?;
        info!("published child presence: {}", present);
    }

    loop {
        tokio::time::sleep(Duration::from_secs(12)).await;

        let event = ChildPresenceEvent {
            present: true,
            confidence: 0.97,
            zone: Some("rear_center".to_string()),
            timestamp_ms: now_ms(),
        };

        publish_with_retry(transport.clone(), &event).await?;
        info!("heartbeat child presence: true");
    }
}

async fn publish_with_retry(
    transport: std::sync::Arc<dyn up_rust::UTransport>,
    payload: &ChildPresenceEvent,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut attempt = 0u8;

    while attempt < 10 {
        let result = publish_json_event(transport.clone(), vss_child_presence_uri(), payload).await;

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
