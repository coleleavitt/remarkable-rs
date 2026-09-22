//! Live MQTT connection test
//!
//! Usage: cargo run -p remarkable-mqtt --bin mqtt-test

use std::path::PathBuf;
use std::time::Duration;

use remarkable_mqtt::{MqttClient, MqttConfig, MqttEvent};
use tracing::{error, info, warn};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("remarkable_mqtt=debug".parse()?)
                .add_directive("rumqttc=debug".parse()?),
        )
        .init();

    // Load tokens from captured_tokens directory
    let tokens_dir = PathBuf::from(std::env::var("HOME")?)
        .join("SiteResearch/remarkable/captured_tokens");

    let device_token = std::fs::read_to_string(tokens_dir.join("device_token_actual.txt"))?
        .trim()
        .to_string();
    let user_token = std::fs::read_to_string(tokens_dir.join("user_token_actual.txt"))?
        .trim()
        .to_string();

    info!(
        device_token_len = device_token.len(),
        user_token_len = user_token.len(),
        "Loaded tokens"
    );

    // Create config
    let config = MqttConfig::from_tokens(&device_token, &user_token)?;
    info!(
        user_id = %config.user_id,
        client_id = %config.client_id,
        broker = %config.broker,
        port = config.port,
        "Created MQTT config"
    );

    // Create client and connect
    let mut client = MqttClient::new(config);

    info!("Connecting to MQTT broker...");
    match client.connect().await {
        Ok(()) => info!("Connected successfully!"),
        Err(e) => {
            error!(?e, "Connection failed");
            return Err(e.into());
        }
    }

    // Subscribe to default topics
    info!("Subscribing to notification topics...");
    client.subscribe_default().await?;
    info!("Subscribed to topics");

    // Poll for events with timeout
    info!("Polling for events (30 second timeout)...");
    let timeout = Duration::from_secs(30);
    let start = std::time::Instant::now();

    while start.elapsed() < timeout {
        match tokio::time::timeout(Duration::from_secs(5), client.poll()).await {
            Ok(Ok(event)) => {
                match &event {
                    MqttEvent::Connected => info!("Event: Connected"),
                    MqttEvent::Disconnected => {
                        warn!("Event: Disconnected");
                        break;
                    }
                    MqttEvent::Subscribed(topic) => info!("Event: Subscribed to {}", topic),
                    MqttEvent::Notification { topic, notification } => {
                        info!(
                            topic = %topic,
                            source = %notification.source_device_id,
                            message = %notification.message,
                            "Event: Notification"
                        );
                    }
                    MqttEvent::SyncComplete { topic, sync } => {
                        info!(
                            topic = %topic,
                            source = %sync.source_device_id,
                            generation = sync.generation,
                            "Event: SyncComplete"
                        );
                    }
                    MqttEvent::Raw { topic, payload } => {
                        info!(
                            topic = %topic,
                            payload_len = payload.len(),
                            payload = %String::from_utf8_lossy(payload),
                            "Event: Raw"
                        );
                    }
                    MqttEvent::Ping => info!("Event: Ping"),
                }
            }
            Ok(Err(e)) => {
                error!(?e, "Poll error");
                break;
            }
            Err(_) => {
                info!("Poll timeout (5s), continuing...");
            }
        }
    }

    // Disconnect
    info!("Disconnecting...");
    client.disconnect().await?;
    info!("Done");

    Ok(())
}
