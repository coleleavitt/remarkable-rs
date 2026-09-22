//! MQTT client for reMarkable real-time sync notifications
//!
//! # Authentication
//! - Username: devicetoken (JWT)
//! - Password: usertoken (JWT)
//!
//! # Broker
//! - Host: vernemq-prod.cloud.remarkable.engineering
//! - Port: 8883 (MQTT over TLS)
//!
//! # Topic Structure
//! ```text
//! user/{user_id}/client/{client_id}/notifications  - General notifications
//! user/{user_id}/client/{client_id}/sync           - Sync completion events
//! user/{user_id}/sync                              - User-wide sync events
//! ```
//!
//! # Example
//! ```ignore
//! use remarkable_mqtt::{MqttClient, MqttConfig};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let device_token = std::env::var("RM_DEVICE_TOKEN")?;
//!     let user_token = std::env::var("RM_USER_TOKEN")?;
//!     let config = MqttConfig::from_tokens(&device_token, &user_token)?;
//!     let mut client = MqttClient::new(config);
//!     client.connect().await?;
//!     
//!     loop {
//!         let event = client.poll().await?;
//!         println!("Event: {:?}", event);
//!     }
//! }
//! ```

mod client;
mod config;
mod error;
mod message;
mod topics;

pub use client::MqttClient;
pub use config::MqttConfig;
pub use error::MqttError;
pub use message::{MqttEvent, Notification, SyncComplete};
pub use topics::Topic;
