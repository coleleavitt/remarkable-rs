//! MQTT message types for reMarkable notifications
//!
//! # Message Format
//!
//! Notifications are JSON objects with a `message` field and metadata:
//!
//! ```json
//! {
//!   "sourceDeviceID": "RM110-218-95935",
//!   "message": "sync-complete for generation 42"
//! }
//! ```

use serde::{Deserialize, Serialize};

use crate::{MqttError, Topic};

/// MQTT event received from broker
#[derive(Debug, Clone)]
pub enum MqttEvent {
    /// Successfully connected to broker
    Connected,

    /// Disconnected from broker
    Disconnected,

    /// Subscription confirmed
    Subscribed(Topic),

    /// Notification message received
    Notification {
        topic: Topic,
        notification: Notification,
    },

    /// Sync complete event
    SyncComplete {
        topic: Topic,
        sync: SyncComplete,
    },

    /// Raw message (unparsed)
    Raw {
        topic: String,
        payload: Vec<u8>,
    },

    /// Keep-alive ping
    Ping,
}

/// Base notification message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    /// Source device ID (e.g., "RM110-218-95935")
    #[serde(rename = "sourceDeviceID")]
    pub source_device_id: String,

    /// Message content
    pub message: String,

    /// Optional timestamp
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
}

impl Notification {
    /// Parse notification from JSON bytes
    pub fn from_bytes(data: &[u8]) -> Result<Self, MqttError> {
        serde_json::from_slice(data)
            .map_err(|e| MqttError::MessageParse(format!("Invalid notification JSON: {}", e)))
    }

    /// Check if this is a sync-complete message
    pub fn is_sync_complete(&self) -> bool {
        self.message.starts_with("sync-complete")
    }

    /// Try to parse as sync complete
    pub fn as_sync_complete(&self) -> Option<SyncComplete> {
        SyncComplete::from_notification(self)
    }
}

/// Sync complete notification
///
/// Message format: `sync-complete for generation {N}`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncComplete {
    /// Source device that completed sync
    #[serde(rename = "sourceDeviceID")]
    pub source_device_id: String,

    /// Sync generation number
    pub generation: u64,
}

impl SyncComplete {
    /// Parse from notification message
    pub fn from_notification(notif: &Notification) -> Option<Self> {
        // Parse "sync-complete for generation 42"
        if !notif.message.starts_with("sync-complete") {
            return None;
        }

        // Extract generation number
        let gen_str = notif
            .message
            .split("generation ")
            .nth(1)?
            .split_whitespace()
            .next()?;

        let generation: u64 = gen_str.parse().ok()?;

        Some(Self {
            source_device_id: notif.source_device_id.clone(),
            generation,
        })
    }
}

/// Screen share signaling message (structure TBD)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenShareSignal {
    /// Signal type (offer, answer, ice-candidate, etc.)
    #[serde(rename = "type")]
    pub signal_type: String,

    /// Signal payload
    pub payload: serde_json::Value,
}

/// Parse MQTT payload into appropriate event type
pub fn parse_mqtt_payload(topic: &str, payload: &[u8]) -> MqttEvent {
    let parsed_topic = Topic::parse(topic).unwrap_or(Topic::Custom(topic.to_string()));

    // Try to parse as notification
    if let Ok(notif) = Notification::from_bytes(payload) {
        // Check if it's a sync-complete
        if let Some(sync) = notif.as_sync_complete() {
            return MqttEvent::SyncComplete {
                topic: parsed_topic,
                sync,
            };
        }

        return MqttEvent::Notification {
            topic: parsed_topic,
            notification: notif,
        };
    }

    // Fall back to raw
    MqttEvent::Raw {
        topic: topic.to_string(),
        payload: payload.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notification_parse() {
        let json = r#"{"sourceDeviceID":"RM110-123","message":"test message"}"#;
        let notif = Notification::from_bytes(json.as_bytes()).unwrap();
        assert_eq!(notif.source_device_id, "RM110-123");
        assert_eq!(notif.message, "test message");
    }

    #[test]
    fn test_sync_complete_parse() {
        let json = r#"{"sourceDeviceID":"RM110-123","message":"sync-complete for generation 42"}"#;
        let notif = Notification::from_bytes(json.as_bytes()).unwrap();
        assert!(notif.is_sync_complete());

        let sync = notif.as_sync_complete().unwrap();
        assert_eq!(sync.generation, 42);
        assert_eq!(sync.source_device_id, "RM110-123");
    }

    #[test]
    fn test_parse_mqtt_payload() {
        let json = r#"{"sourceDeviceID":"RM110","message":"sync-complete for generation 100"}"#;
        let event = parse_mqtt_payload("user/abc/sync", json.as_bytes());

        match event {
            MqttEvent::SyncComplete { sync, .. } => {
                assert_eq!(sync.generation, 100);
            }
            _ => panic!("Expected SyncComplete event"),
        }
    }
}
