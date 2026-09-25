//! MQTT client for reMarkable real-time sync notifications
//!
//! # Authentication
//!
//! MQTT authentication uses device tokens from xochitl.conf:
//! - Username: devicetoken (JWT)
//! - Password: usertoken (JWT)
//!
//! # Broker
//!
//! - Host: vernemq-prod.cloud.remarkable.engineering
//! - Port: 443 (WebSocket over TLS)
//!
//! # Topic Structure
//!
//! ```text
//! user/{user_id}/sync                              - User-wide sync events
//! user/{user_id}/client/{client_id}/notifications  - General notifications
//! user/{user_id}/client/{client_id}/sync           - Client-specific sync
//! remarkable/screenshare/signaling/user/{user_id}/client/{client_id}
//!                                                  - Screen share signaling (publish)
//! ```
//!
//! # Features
//!
//! - Token extraction via SSH from device
//! - Auto-reconnection with exponential backoff
//! - Screen share WebRTC signaling
//! - Sync event integration
//!
//! # Example
//!
//! ```ignore
//! use remarkable_mqtt::{MqttConfig, ReconnectingClient, MqttEvent};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let device_token = std::env::var("RM_DEVICE_TOKEN")?;
//!     let user_token = std::env::var("RM_USER_TOKEN")?;
//!     let config = MqttConfig::from_tokens(&device_token, &user_token)?;
//!     
//!     let mut client = ReconnectingClient::new(config);
//!     client.connect().await?;
//!     client.subscribe_default().await?;
//!     
//!     loop {
//!         match client.poll().await? {
//!             MqttEvent::SyncComplete { sync, .. } => {
//!                 println!("Sync complete: generation {}", sync.generation);
//!             }
//!             _ => {}
//!         }
//!     }
//! }
//! ```

mod client;
mod config;
mod error;
mod message;
mod reconnect;
pub mod screenshare;
pub mod ssh;
pub mod sync_events;
mod topics;

pub use client::{spawn_listener, MqttClient};
pub use config::{MqttConfig, DEFAULT_BROKER, DEFAULT_PORT};
pub use error::MqttError;
pub use message::{parse_mqtt_payload, MqttEvent, Notification, SyncComplete};
pub use reconnect::{BackoffConfig, ConnectionState, EventHandler, ReconnectingClient};
pub use screenshare::{PeerMessage, SignalingEvent, SignalingRequest, WebRtcMessage};
pub use topics::{default_subscriptions, Topic};
