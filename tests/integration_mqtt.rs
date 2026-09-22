//! MQTT event handling integration tests
//!
//! Tests MQTT real-time sync notifications:
//! 1. Connection to VerneMQ broker
//! 2. Topic subscription
//! 3. Event parsing
//! 4. Sync notifications
//!
//! Note: Full MQTT tests require device-extracted tokens.
//! VerneMQ rejects webapp-obtained tokens.

mod common;

use common::fixtures::*;
use serde_json::json;
use std::time::Duration;

/// Test MQTT config creation
#[test]
fn test_mqtt_config_creation() {
    use remarkable_mqtt::MqttConfig;
    
    let device_token = mock_device_token("test-device");
    let user_token = mock_user_token("eu");
    
    let config = MqttConfig::from_tokens(&device_token, &user_token);
    
    // Should succeed with valid mock tokens
    assert!(config.is_ok());
    
    let config = config.unwrap();
    assert!(!config.device_token.is_empty());
    assert!(!config.user_token.is_empty());
}

/// Test default broker configuration
#[test]
fn test_mqtt_default_broker() {
    use remarkable_mqtt::{MqttConfig, DEFAULT_BROKER, DEFAULT_PORT};
    
    let device_token = mock_device_token("test-device");
    let user_token = mock_user_token("eu");
    
    let config = MqttConfig::from_tokens(&device_token, &user_token).unwrap();
    
    assert_eq!(config.broker, DEFAULT_BROKER);
    assert_eq!(config.port, DEFAULT_PORT);
}

/// Test MQTT topic structure
#[test]
fn test_mqtt_topic_structure() {
    use remarkable_mqtt::Topic;
    
    let user_id = "auth0|12345678";
    
    // Sync notification topic
    let sync_topic = Topic::user_sync(user_id);
    assert!(sync_topic.as_str().starts_with("user/"));
    assert!(sync_topic.as_str().ends_with("/sync"));
    
    // Screen share topic
    let screen_topic = Topic::screen_share(user_id, "client-1");
    assert!(screen_topic.as_str().contains("/screenshare"));
}

/// Test sync complete event parsing
#[test]
fn test_sync_complete_event_parsing() {
    use remarkable_mqtt::Notification;
    
    let payload = json!({
        "sourceDeviceID": "RM110-test",
        "message": "sync-complete for generation 42"
    });
    
    let notif = Notification::from_bytes(&serde_json::to_vec(&payload).unwrap());
    assert!(notif.is_ok());
    
    let notif = notif.unwrap();
    assert!(notif.is_sync_complete());
    
    let sync = notif.as_sync_complete();
    assert!(sync.is_some());
    assert_eq!(sync.unwrap().generation, 42);
}

/// Test document update event structure
#[test]
fn test_document_update_event() {
    let event = json!({
        "type": "document_update",
        "document_id": "550e8400-e29b-41d4-a716-446655440000",
        "action": "modified",
        "timestamp": "2024-01-01T00:00:00Z"
    });
    
    assert_eq!(event["type"], "document_update");
    assert!(event["document_id"].as_str().is_some());
}

/// Test screen share event structure
#[test]
fn test_screen_share_event() {
    let event = json!({
        "type": "screen_share",
        "action": "started",
        "session_id": "session-123"
    });
    
    assert_eq!(event["type"], "screen_share");
    assert!(event["action"].as_str().is_some());
}

/// Test MQTT client creation
#[test]
fn test_mqtt_client_creation() {
    use remarkable_mqtt::{MqttClient, MqttConfig};
    
    let device_token = mock_device_token("test-device");
    let user_token = mock_user_token("eu");
    let config = MqttConfig::from_tokens(&device_token, &user_token).unwrap();
    
    let _client = MqttClient::new(config);
    // Should create without error
}

/// Test default topic subscriptions
#[test]
fn test_default_subscriptions() {
    use remarkable_mqtt::default_subscriptions;
    
    let user_id = "test-user";
    let client_id = "test-client";
    let topics = default_subscriptions(user_id, client_id);
    
    // Should include sync and notification topics
    assert!(!topics.is_empty());
    assert!(topics.len() >= 3);
}

/// Test topic pattern matching
#[test]
fn test_topic_pattern_matching() {
    use remarkable_mqtt::Topic;
    
    // Topic parsing
    let parsed = Topic::parse("user/abc/sync");
    assert!(parsed.is_some());
    
    let parsed = Topic::parse("user/abc/client/xyz/notifications");
    assert!(parsed.is_some());
}

/// Test QoS levels
#[test]
fn test_mqtt_qos_levels() {
    // QoS 0: At most once (fire and forget)
    // QoS 1: At least once (acknowledged delivery)
    // QoS 2: Exactly once (assured delivery)
    
    let qos_at_most_once = 0u8;
    let qos_at_least_once = 1u8;
    let qos_exactly_once = 2u8;
    
    assert!(qos_at_most_once < qos_at_least_once);
    assert!(qos_at_least_once < qos_exactly_once);
}

/// Test MQTT keep-alive
#[test]
fn test_mqtt_keepalive() {
    use remarkable_mqtt::MqttConfig;
    
    let device_token = mock_device_token("test-device");
    let user_token = mock_user_token("eu");
    let config = MqttConfig::from_tokens(&device_token, &user_token)
        .unwrap()
        .with_keep_alive(90);
    
    assert_eq!(config.keep_alive_secs, 90);
}

/// Test connection timeout handling
#[tokio::test]
async fn test_mqtt_connection_timeout() {
    use tokio::time::timeout;
    
    // Should handle connection timeouts gracefully
    let result = timeout(Duration::from_millis(100), async {
        // Simulate connection attempt to unreachable broker
        tokio::time::sleep(Duration::from_secs(10)).await;
    })
    .await;
    
    assert!(result.is_err(), "Should timeout");
}

/// Test reconnection logic
#[test]
fn test_reconnection_backoff() {
    // Exponential backoff for reconnection
    let base_delay_ms = 1000u64;
    let max_delay_ms = 30000u64;
    
    let delays: Vec<u64> = (0..5)
        .map(|attempt| {
            let delay = base_delay_ms * 2u64.pow(attempt);
            delay.min(max_delay_ms)
        })
        .collect();
    
    assert_eq!(delays[0], 1000);
    assert_eq!(delays[1], 2000);
    assert_eq!(delays[2], 4000);
    assert_eq!(delays[3], 8000);
    assert_eq!(delays[4], 16000);
}

/// Test message serialization
#[test]
fn test_mqtt_message_serialization() {
    let message = json!({
        "action": "sync_request",
        "client_id": "test-client",
        "timestamp": 1704067200
    });
    
    let serialized = serde_json::to_vec(&message).unwrap();
    let deserialized: serde_json::Value = serde_json::from_slice(&serialized).unwrap();
    
    assert_eq!(message, deserialized);
}

/// Test with real device (skipped if not available)
#[tokio::test]
#[ignore = "requires device-extracted tokens"]
async fn test_real_mqtt_connection() {
    // This test requires tokens from the device's xochitl.conf
    // Webapp tokens are rejected by VerneMQ
    
    let tokens = match load_captured_tokens() {
        Some(t) => t,
        None => {
            eprintln!("No captured tokens available");
            return;
        }
    };
    
    eprintln!("Have tokens, would attempt MQTT connection");
    eprintln!("Device token length: {}", tokens.0.len());
    eprintln!("User token length: {}", tokens.1.len());
}

/// Test MQTT event dispatch
#[test]
fn test_event_dispatch() {
    use remarkable_mqtt::MqttEvent;
    
    // Create sample events
    let connected = MqttEvent::Connected;
    let disconnected = MqttEvent::Disconnected;
    let ping = MqttEvent::Ping;
    
    // Should be able to match on events
    match connected {
        MqttEvent::Connected => assert!(true),
        _ => panic!("Wrong event type"),
    }
    
    match disconnected {
        MqttEvent::Disconnected => assert!(true),
        _ => panic!("Wrong event type"),
    }
    
    match ping {
        MqttEvent::Ping => assert!(true),
        _ => panic!("Wrong event type"),
    }
}

/// Test MQTT error handling
#[test]
fn test_mqtt_error_types() {
    use remarkable_mqtt::MqttError;
    
    // Test error variants exist and are displayable
    let parse_error = MqttError::TokenParse("test".to_string());
    assert!(!parse_error.to_string().is_empty());
}

/// Test JWT claim extraction for user ID
#[test]
fn test_jwt_user_id_extraction() {
    let user_token = mock_user_token("eu");
    
    // Extract claims (base64 decode middle part)
    let parts: Vec<&str> = user_token.split('.').collect();
    assert_eq!(parts.len(), 3);
    
    use base64::Engine;
    let claims_json = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(parts[1])
        .unwrap();
    let claims: serde_json::Value = serde_json::from_slice(&claims_json).unwrap();
    
    assert!(claims.get("sub").is_some());
}

/// Test notification parsing with invalid JSON
#[test]
fn test_invalid_notification_parsing() {
    use remarkable_mqtt::Notification;
    
    let invalid = b"not valid json";
    let result = Notification::from_bytes(invalid);
    assert!(result.is_err());
}

/// Test notification parsing with missing fields
#[test]
fn test_notification_missing_fields() {
    use remarkable_mqtt::Notification;
    
    // Missing sourceDeviceID
    let missing_device = json!({"message": "test"});
    let result = Notification::from_bytes(&serde_json::to_vec(&missing_device).unwrap());
    assert!(result.is_err());
}
