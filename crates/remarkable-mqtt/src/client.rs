//! MQTT client implementation using rumqttc
//!
//! # Connection Flow
//!
//! 1. Create TLS connection to vernemq-prod.cloud.remarkable.engineering:443
//! 2. Authenticate with username=devicetoken, password=usertoken
//! 3. Subscribe to sync notification topics
//! 4. Poll for incoming events
//!
//! # Authentication Note
//!
//! MQTT authentication requires tokens extracted from the device's xochitl.conf
//! file (via SSH access to the device). Tokens obtained through the webapp
//! pairing flow are rejected with "Not authorized" - VerneMQ validates
//! device-specific claims that differ from webapp tokens.
//!
//! # Example
//!
//! ```ignore
//! use remarkable_mqtt::{MqttClient, MqttConfig, MqttEvent};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Tokens must be from device xochitl.conf, not webapp pairing
//!     let device_token = std::env::var("RM_DEVICE_TOKEN")?;
//!     let user_token = std::env::var("RM_USER_TOKEN")?;
//!     let config = MqttConfig::from_tokens(&device_token, &user_token)?;
//!     let mut client = MqttClient::new(config);
//!     
//!     client.connect().await?;
//!     client.subscribe_default().await?;
//!     
//!     loop {
//!         match client.poll().await? {
//!             MqttEvent::SyncComplete(sync) => {
//!                 println!("Sync complete: generation {}", sync.generation);
//!             }
//!             _ => {}
//!         }
//!     }
//! }
//! ```

use std::time::Duration;

use rumqttc::{AsyncClient, Event, EventLoop, MqttOptions, Packet, QoS, Transport};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::{
    message::{parse_mqtt_payload, MqttEvent},
    topics::default_subscriptions,
    MqttConfig, MqttError, Topic,
};

/// MQTT client for reMarkable real-time sync
pub struct MqttClient {
    config: MqttConfig,
    client: Option<AsyncClient>,
    eventloop: Option<EventLoop>,
}

impl MqttClient {
    /// Create new MQTT client with configuration
    pub fn new(config: MqttConfig) -> Self {
        Self {
            config,
            client: None,
            eventloop: None,
        }
    }

    /// Connect to the MQTT broker
    ///
    /// Uses TLS with:
    /// - Username: devicetoken
    /// - Password: usertoken
    pub async fn connect(&mut self) -> Result<(), MqttError> {
        info!(
            broker = %self.config.broker,
            port = self.config.port,
            client_id = %self.config.client_id,
            "Connecting to MQTT broker"
        );

        // Create MQTT options
        let mut mqtt_options = MqttOptions::new(
            &self.config.client_id,
            &self.config.broker,
            self.config.port,
        );

        // Set credentials: username = devicetoken, password = usertoken
        mqtt_options.set_credentials(&self.config.device_token, &self.config.user_token);

        // Set keep-alive
        mqtt_options.set_keep_alive(Duration::from_secs(self.config.keep_alive_secs));

        // Enable TLS with native roots
        let tls_config = rumqttc::TlsConfiguration::default();
        mqtt_options.set_transport(Transport::Tls(tls_config));

        // Create client and event loop
        let (client, eventloop) = AsyncClient::new(mqtt_options, 10);

        self.client = Some(client);
        self.eventloop = Some(eventloop);

        // Wait for connection to establish by polling once
        if let Some(ref mut eventloop) = self.eventloop {
            match eventloop.poll().await {
                Ok(Event::Incoming(Packet::ConnAck(ack))) => {
                    info!(?ack, "Connected to MQTT broker");
                    Ok(())
                }
                Ok(event) => {
                    debug!(?event, "Received event while connecting");
                    Ok(())
                }
                Err(e) => {
                    error!(?e, "Connection failed");
                    Err(MqttError::Connection(e.to_string()))
                }
            }
        } else {
            Err(MqttError::Disconnected)
        }
    }

    /// Subscribe to default notification topics
    pub async fn subscribe_default(&self) -> Result<(), MqttError> {
        let topics = default_subscriptions(&self.config.user_id, &self.config.client_id);
        for topic in topics {
            self.subscribe(&topic).await?;
        }
        Ok(())
    }

    /// Subscribe to a specific topic
    pub async fn subscribe(&self, topic: &Topic) -> Result<(), MqttError> {
        let client = self.client.as_ref().ok_or(MqttError::Disconnected)?;

        let topic_str = topic.as_str();
        info!(topic = %topic_str, "Subscribing to topic");

        client
            .subscribe(&topic_str, QoS::AtLeastOnce)
            .await
            .map_err(|e| MqttError::Subscribe(e.to_string()))?;

        Ok(())
    }

    /// Poll for next MQTT event
    ///
    /// This is the main event loop driver. Call this repeatedly to receive events.
    pub async fn poll(&mut self) -> Result<MqttEvent, MqttError> {
        let eventloop = self.eventloop.as_mut().ok_or(MqttError::Disconnected)?;

        loop {
            match eventloop.poll().await {
                Ok(Event::Incoming(packet)) => {
                    match packet {
                        Packet::Publish(publish) => {
                            debug!(
                                topic = %publish.topic,
                                payload_len = publish.payload.len(),
                                "Received message"
                            );
                            return Ok(parse_mqtt_payload(&publish.topic, &publish.payload));
                        }
                        Packet::SubAck(ack) => {
                            debug!(?ack, "Subscription acknowledged");
                            // Continue polling
                        }
                        Packet::ConnAck(ack) => {
                            info!(?ack, "Connection acknowledged");
                            return Ok(MqttEvent::Connected);
                        }
                        Packet::PingResp => {
                            debug!("Ping response");
                            return Ok(MqttEvent::Ping);
                        }
                        Packet::Disconnect => {
                            warn!("Disconnected by broker");
                            return Ok(MqttEvent::Disconnected);
                        }
                        _ => {
                            debug!(?packet, "Other packet");
                            // Continue polling
                        }
                    }
                }
                Ok(Event::Outgoing(outgoing)) => {
                    debug!(?outgoing, "Outgoing event");
                    // Continue polling
                }
                Err(e) => {
                    error!(?e, "Event loop error");
                    return Err(MqttError::ConnectionError(e));
                }
            }
        }
    }

    /// Publish a message to a topic
    pub async fn publish(&self, topic: &Topic, payload: &[u8]) -> Result<(), MqttError> {
        let client = self.client.as_ref().ok_or(MqttError::Disconnected)?;

        client
            .publish(topic.as_str(), QoS::AtLeastOnce, false, payload)
            .await
            .map_err(|e| MqttError::Publish(e.to_string()))?;

        Ok(())
    }

    /// Disconnect from broker
    pub async fn disconnect(&self) -> Result<(), MqttError> {
        if let Some(client) = &self.client {
            client
                .disconnect()
                .await
                .map_err(|e| MqttError::Connection(e.to_string()))?;
        }
        Ok(())
    }

    /// Get the user ID
    pub fn user_id(&self) -> &str {
        &self.config.user_id
    }

    /// Get the client ID
    pub fn client_id(&self) -> &str {
        &self.config.client_id
    }

    /// Check if connected
    pub fn is_connected(&self) -> bool {
        self.client.is_some()
    }
}

/// Create a simple notification listener that runs in background
///
/// Returns a receiver channel for events
pub async fn spawn_listener(
    config: MqttConfig,
) -> Result<mpsc::Receiver<MqttEvent>, MqttError> {
    let (tx, rx) = mpsc::channel(100);

    let mut client = MqttClient::new(config);
    client.connect().await?;
    client.subscribe_default().await?;

    tokio::spawn(async move {
        loop {
            match client.poll().await {
                Ok(event) => {
                    if tx.send(event).await.is_err() {
                        break;
                    }
                }
                Err(e) => {
                    error!(?e, "Listener error");
                    break;
                }
            }
        }
    });

    Ok(rx)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Integration test - requires valid tokens
    // Run with: cargo test --package remarkable-mqtt -- --ignored
    #[tokio::test]
    #[ignore]
    async fn test_live_connection() {
        let device_token = std::env::var("REMARKABLE_DEVICE_TOKEN").unwrap();
        let user_token = std::env::var("REMARKABLE_USER_TOKEN").unwrap();

        let config = MqttConfig::from_tokens(&device_token, &user_token).unwrap();
        let mut client = MqttClient::new(config);

        client.connect().await.unwrap();
        client.subscribe_default().await.unwrap();

        // Poll for a few events
        for _ in 0..5 {
            match tokio::time::timeout(Duration::from_secs(5), client.poll()).await {
                Ok(Ok(event)) => println!("Event: {:?}", event),
                Ok(Err(e)) => eprintln!("Error: {}", e),
                Err(_) => println!("Timeout"),
            }
        }

        client.disconnect().await.unwrap();
    }
}
