//! MQTT-based WebRTC Signaling for reMarkable Screen Share
//!
//! # Protocol
//! 
//! 1. Client connects to MQTT broker with device/user tokens
//! 2. Client subscribes to signaling topic
//! 3. Client sends request-offer message
//! 4. Device responds with SDP offer
//! 5. Client sends SDP answer
//! 6. ICE candidates exchanged bidirectionally
//! 7. WebRTC DataChannel established
//!
//! # MQTT Topics
//! 
//! ```text
//! remarkable/screenshare/signaling/user/{user_id}
//! remarkable/screenshare/signaling/user/{user_id}/client/{client_id}
//! remarkable/screenshare/signaling/user/{user_id}/client/{client_id}/room/{room_id}
//! ```

use std::time::Duration;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rumqttc::{AsyncClient, Event, EventLoop, MqttOptions, Packet, QoS, Transport};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, error, info};
use uuid::Uuid;

/// Default MQTT broker
pub const MQTT_BROKER: &str = "vernemq-prod.cloud.remarkable.engineering";
pub const MQTT_PORT: u16 = 443;

/// Signaling message types
pub const MSG_REQUEST_OFFER: &str = "request-offer";
pub const MSG_OFFER: &str = "offer";
pub const MSG_ANSWER: &str = "answer";
pub const MSG_CANDIDATE: &str = "candidate";
pub const MSG_ROOM_CREATED: &str = "room-created";
pub const MSG_CONNECTION_DECLINED: &str = "connectionDeclined";

/// Signaling errors
#[derive(Error, Debug)]
pub enum SignalingError {
    #[error("MQTT connection failed: {0}")]
    Connection(String),

    #[error("Authentication failed: {0}")]
    Auth(String),

    #[error("Token parse error: {0}")]
    TokenParse(String),

    #[error("Subscription failed: {0}")]
    Subscribe(String),

    #[error("Publish failed: {0}")]
    Publish(String),

    #[error("Message parse error: {0}")]
    MessageParse(String),

    #[error("Connection declined by device")]
    ConnectionDeclined,

    #[error("Timeout waiting for {0}")]
    Timeout(String),

    #[error("Disconnected")]
    Disconnected,

    #[error("MQTT error: {0}")]
    Mqtt(#[from] rumqttc::ClientError),

    #[error("Connection error: {0}")]
    ConnectionError(#[from] rumqttc::ConnectionError),
}

/// Signaling connection state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalingState {
    Disconnected,
    Connecting,
    Connected,
    WaitingForOffer,
    Negotiating,
    Established,
    Error,
}

/// WebRTC signaling message
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignalingMessage {
    #[serde(rename = "type")]
    pub msg_type: String,

    pub id: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub sdp: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub sdp_mid: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub sdp_m_line_index: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub room_id: Option<String>,

    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl SignalingMessage {
    /// Create request-offer message
    pub fn request_offer(client_id: &str) -> Self {
        Self {
            msg_type: MSG_REQUEST_OFFER.to_string(),
            id: client_id.to_string(),
            sdp: None,
            candidate: None,
            sdp_mid: None,
            sdp_m_line_index: None,
            room_id: None,
            extra: serde_json::Map::new(),
        }
    }

    /// Create SDP answer message
    pub fn answer(client_id: &str, sdp: &str) -> Self {
        Self {
            msg_type: MSG_ANSWER.to_string(),
            id: client_id.to_string(),
            sdp: Some(sdp.to_string()),
            candidate: None,
            sdp_mid: None,
            sdp_m_line_index: None,
            room_id: None,
            extra: serde_json::Map::new(),
        }
    }

    /// Create ICE candidate message
    pub fn candidate(client_id: &str, candidate: &str, sdp_mid: &str, index: u32) -> Self {
        Self {
            msg_type: MSG_CANDIDATE.to_string(),
            id: client_id.to_string(),
            sdp: None,
            candidate: Some(candidate.to_string()),
            sdp_mid: Some(sdp_mid.to_string()),
            sdp_m_line_index: Some(index),
            room_id: None,
            extra: serde_json::Map::new(),
        }
    }

    /// Check if this is an offer
    pub fn is_offer(&self) -> bool {
        self.msg_type == MSG_OFFER
    }

    /// Check if this is a candidate
    pub fn is_candidate(&self) -> bool {
        self.msg_type == MSG_CANDIDATE
    }

    /// Check if connection was declined
    pub fn is_declined(&self) -> bool {
        self.msg_type == MSG_CONNECTION_DECLINED
    }
}

/// Signaling event from MQTT
#[derive(Debug, Clone)]
pub enum SignalingEvent {
    Connected,
    Disconnected,
    Offer(SignalingMessage),
    Candidate(SignalingMessage),
    RoomCreated { room_id: String },
    Declined,
    Error(String),
}

/// MQTT signaling client configuration
#[derive(Debug, Clone)]
pub struct SignalingConfig {
    pub broker: String,
    pub port: u16,
    pub device_token: String,
    pub user_token: String,
    pub user_id: String,
    pub client_id: String,
}

impl SignalingConfig {
    /// Create config from tokens
    pub fn from_tokens(device_token: &str, user_token: &str) -> Result<Self, SignalingError> {
        let user_id = extract_user_id(user_token)
            .ok_or_else(|| SignalingError::TokenParse("Failed to extract user_id".into()))?;

        let client_id = format!("screenshare-{}", Uuid::new_v4().as_simple());

        Ok(Self {
            broker: MQTT_BROKER.to_string(),
            port: MQTT_PORT,
            device_token: device_token.to_string(),
            user_token: user_token.to_string(),
            user_id,
            client_id,
        })
    }
}

/// Extract user ID from JWT token
fn extract_user_id(token: &str) -> Option<String> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }

    let payload = parts[1];
    let decoded = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let json: serde_json::Value = serde_json::from_slice(&decoded).ok()?;

    json.get("auth0-userid")
        .and_then(|v| v.as_str())
        .or_else(|| {
            json.get("auth0-profile")
                .and_then(|p| p.get("UserID"))
                .and_then(|v| v.as_str())
        })
        .or_else(|| json.get("sub").and_then(|v| v.as_str()))
        .map(|s| s.to_string())
}

/// MQTT signaling client for WebRTC
pub struct MqttSignaling {
    config: SignalingConfig,
    client: Option<AsyncClient>,
    eventloop: Option<EventLoop>,
    state: SignalingState,
    room_id: Option<String>,
}

impl MqttSignaling {
    /// Create new signaling client
    pub fn new(config: SignalingConfig) -> Self {
        Self {
            config,
            client: None,
            eventloop: None,
            state: SignalingState::Disconnected,
            room_id: None,
        }
    }

    /// Get current state
    pub fn state(&self) -> SignalingState {
        self.state
    }

    /// Get room ID if set
    pub fn room_id(&self) -> Option<&str> {
        self.room_id.as_deref()
    }

    /// Get client ID
    pub fn client_id(&self) -> &str {
        &self.config.client_id
    }

    /// Connect to MQTT broker
    pub async fn connect(&mut self) -> Result<(), SignalingError> {
        self.state = SignalingState::Connecting;

        info!(
            broker = %self.config.broker,
            port = self.config.port,
            client_id = %self.config.client_id,
            "Connecting to MQTT broker"
        );

        let mut mqtt_options = MqttOptions::new(
            &self.config.client_id,
            &self.config.broker,
            self.config.port,
        );

        // Auth: username = devicetoken, password = usertoken
        mqtt_options.set_credentials(&self.config.device_token, &self.config.user_token);
        mqtt_options.set_keep_alive(Duration::from_secs(60));

        // TLS
        let tls_config = rumqttc::TlsConfiguration::default();
        mqtt_options.set_transport(Transport::Tls(tls_config));

        let (client, eventloop) = AsyncClient::new(mqtt_options, 10);
        self.client = Some(client);
        self.eventloop = Some(eventloop);

        // Wait for ConnAck
        if let Some(ref mut eventloop) = self.eventloop {
            match eventloop.poll().await {
                Ok(Event::Incoming(Packet::ConnAck(ack))) => {
                    info!(?ack, "Connected to MQTT broker");
                    self.state = SignalingState::Connected;
                    Ok(())
                }
                Ok(event) => {
                    debug!(?event, "Unexpected event during connect");
                    Ok(())
                }
                Err(e) => {
                    self.state = SignalingState::Error;
                    Err(SignalingError::Connection(e.to_string()))
                }
            }
        } else {
            Err(SignalingError::Disconnected)
        }
    }

    /// Subscribe to signaling topics
    pub async fn subscribe(&self) -> Result<(), SignalingError> {
        let client = self.client.as_ref().ok_or(SignalingError::Disconnected)?;

        // Subscribe to user signaling topic with wildcard
        let topic = format!(
            "remarkable/screenshare/signaling/user/{}/#",
            self.config.user_id
        );

        info!(topic = %topic, "Subscribing to signaling topic");

        client
            .subscribe(&topic, QoS::AtLeastOnce)
            .await
            .map_err(|e| SignalingError::Subscribe(e.to_string()))?;

        Ok(())
    }

    /// Send request-offer to initiate screen share
    pub async fn request_offer(&mut self) -> Result<(), SignalingError> {
        let client = self.client.as_ref().ok_or(SignalingError::Disconnected)?;

        let msg = SignalingMessage::request_offer(&self.config.client_id);
        let topic = format!(
            "remarkable/screenshare/signaling/user/{}",
            self.config.user_id
        );

        let payload = serde_json::to_string(&msg)
            .map_err(|e| SignalingError::MessageParse(e.to_string()))?;

        info!(topic = %topic, "Sending request-offer");

        client
            .publish(&topic, QoS::AtLeastOnce, false, payload.as_bytes())
            .await
            .map_err(|e| SignalingError::Publish(e.to_string()))?;

        self.state = SignalingState::WaitingForOffer;
        Ok(())
    }

    /// Send SDP answer
    pub async fn send_answer(&self, sdp: &str) -> Result<(), SignalingError> {
        let client = self.client.as_ref().ok_or(SignalingError::Disconnected)?;
        let room_id = self.room_id.as_ref().ok_or(SignalingError::Disconnected)?;

        let msg = SignalingMessage::answer(&self.config.client_id, sdp);
        let topic = format!(
            "remarkable/screenshare/signaling/user/{}/client/{}/room/{}",
            self.config.user_id, self.config.client_id, room_id
        );

        let payload = serde_json::to_string(&msg)
            .map_err(|e| SignalingError::MessageParse(e.to_string()))?;

        info!(topic = %topic, "Sending SDP answer");

        client
            .publish(&topic, QoS::AtLeastOnce, false, payload.as_bytes())
            .await
            .map_err(|e| SignalingError::Publish(e.to_string()))?;

        Ok(())
    }

    /// Send ICE candidate
    pub async fn send_candidate(
        &self,
        candidate: &str,
        sdp_mid: &str,
        index: u32,
    ) -> Result<(), SignalingError> {
        let client = self.client.as_ref().ok_or(SignalingError::Disconnected)?;
        let room_id = self.room_id.as_ref().ok_or(SignalingError::Disconnected)?;

        let msg = SignalingMessage::candidate(&self.config.client_id, candidate, sdp_mid, index);
        let topic = format!(
            "remarkable/screenshare/signaling/user/{}/client/{}/room/{}",
            self.config.user_id, self.config.client_id, room_id
        );

        let payload = serde_json::to_string(&msg)
            .map_err(|e| SignalingError::MessageParse(e.to_string()))?;

        debug!(topic = %topic, "Sending ICE candidate");

        client
            .publish(&topic, QoS::AtLeastOnce, false, payload.as_bytes())
            .await
            .map_err(|e| SignalingError::Publish(e.to_string()))?;

        Ok(())
    }

    /// Poll for next signaling event
    pub async fn poll(&mut self) -> Result<SignalingEvent, SignalingError> {
        let eventloop = self.eventloop.as_mut().ok_or(SignalingError::Disconnected)?;

        loop {
            match eventloop.poll().await {
                Ok(Event::Incoming(Packet::Publish(publish))) => {
                    debug!(
                        topic = %publish.topic,
                        payload_len = publish.payload.len(),
                        "Received signaling message"
                    );

                    // Parse message
                    let msg: SignalingMessage = serde_json::from_slice(&publish.payload)
                        .map_err(|e| SignalingError::MessageParse(e.to_string()))?;

                    // Handle message type
                    if msg.is_declined() {
                        self.state = SignalingState::Error;
                        return Ok(SignalingEvent::Declined);
                    }

                    if msg.msg_type == MSG_ROOM_CREATED {
                        if let Some(room_id) = &msg.room_id {
                            self.room_id = Some(room_id.clone());
                            return Ok(SignalingEvent::RoomCreated {
                                room_id: room_id.clone(),
                            });
                        }
                    }

                    if msg.is_offer() {
                        if let Some(room_id) = &msg.room_id {
                            self.room_id = Some(room_id.clone());
                        }
                        self.state = SignalingState::Negotiating;
                        return Ok(SignalingEvent::Offer(msg));
                    }

                    if msg.is_candidate() {
                        return Ok(SignalingEvent::Candidate(msg));
                    }

                    debug!(msg_type = %msg.msg_type, "Ignoring message");
                }
                Ok(Event::Incoming(Packet::ConnAck(_))) => {
                    self.state = SignalingState::Connected;
                    return Ok(SignalingEvent::Connected);
                }
                Ok(Event::Incoming(Packet::Disconnect)) => {
                    self.state = SignalingState::Disconnected;
                    return Ok(SignalingEvent::Disconnected);
                }
                Ok(Event::Incoming(Packet::SubAck(_))) => {
                    debug!("Subscription acknowledged");
                }
                Ok(Event::Incoming(Packet::PingResp)) => {
                    debug!("Ping response");
                }
                Ok(Event::Outgoing(_)) => {}
                Ok(other) => {
                    debug!(?other, "Other MQTT event");
                }
                Err(e) => {
                    error!(?e, "MQTT error");
                    self.state = SignalingState::Error;
                    return Err(SignalingError::ConnectionError(e));
                }
            }
        }
    }

    /// Disconnect from broker
    pub async fn disconnect(&self) -> Result<(), SignalingError> {
        if let Some(client) = &self.client {
            client
                .disconnect()
                .await
                .map_err(|e| SignalingError::Connection(e.to_string()))?;
        }
        Ok(())
    }
}



#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_user_id() {
        let payload = r#"{"auth0-userid":"auth0|12345"}"#;
        let encoded = URL_SAFE_NO_PAD.encode(payload.as_bytes());
        let token = format!("header.{}.sig", encoded);
        assert_eq!(extract_user_id(&token), Some("auth0|12345".to_string()));
    }

    #[test]
    fn test_request_offer_message() {
        let msg = SignalingMessage::request_offer("test-client");
        assert_eq!(msg.msg_type, MSG_REQUEST_OFFER);
        assert_eq!(msg.id, "test-client");
    }

    #[test]
    fn test_answer_message() {
        let msg = SignalingMessage::answer("test-client", "v=0
...");
        assert_eq!(msg.msg_type, MSG_ANSWER);
        assert!(msg.sdp.is_some());
    }
}
