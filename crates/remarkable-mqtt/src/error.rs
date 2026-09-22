//! Error types for MQTT operations

use thiserror::Error;

/// MQTT operation errors
#[derive(Error, Debug)]
pub enum MqttError {
    /// Connection to broker failed
    #[error("Connection failed: {0}")]
    Connection(String),

    /// Authentication failed (invalid tokens)
    #[error("Authentication failed: {0}")]
    Auth(String),

    /// Failed to parse JWT token
    #[error("Token parse error: {0}")]
    TokenParse(String),

    /// Subscription failed
    #[error("Subscribe failed: {0}")]
    Subscribe(String),

    /// Publish failed
    #[error("Publish failed: {0}")]
    Publish(String),

    /// Message parse error
    #[error("Message parse error: {0}")]
    MessageParse(String),

    /// Client disconnected
    #[error("Client disconnected")]
    Disconnected,

    /// Timeout waiting for response
    #[error("Timeout")]
    Timeout,

    /// MQTT protocol error
    #[error("MQTT error: {0}")]
    Mqtt(#[from] rumqttc::ClientError),

    /// MQTT connection error
    #[error("Connection error: {0}")]
    ConnectionError(#[from] rumqttc::ConnectionError),
}

impl MqttError {
    /// Check if error is recoverable (should retry)
    pub fn is_recoverable(&self) -> bool {
        matches!(self, Self::Connection(_) | Self::Timeout)
    }
}
