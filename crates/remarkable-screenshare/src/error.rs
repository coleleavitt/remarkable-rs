//! Error types for remarkable-screenshare

use thiserror::Error;

/// Main error type
#[derive(Error, Debug)]
pub enum Error {
    #[error("MQTT connection error: {0}")]
    MqttConnection(String),
    
    #[error("MQTT signaling error: {0}")]
    MqttSignaling(String),
    
    #[error("WebRTC error: {0}")]
    WebRtc(String),
    
    #[error("WebRTC peer connection failed: {0}")]
    PeerConnection(String),
    
    #[error("WebRTC ICE negotiation failed: {0}")]
    IceNegotiation(String),
    
    #[error("USB connection error: {0}")]
    UsbConnection(String),
    
    #[error("SSH error: {0}")]
    Ssh(String),
    
    #[error("RFB protocol error: {0}")]
    RfbProtocol(String),
    
    #[error("Framebuffer error: {0}")]
    Framebuffer(String),
    
    #[error("Recording error: {0}")]
    Recording(String),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Server error: {0}")]
    Server(String),
    
    #[error("Device not in screen share mode")]
    DeviceNotReady,
    
    #[error("Timeout: {0}")]
    Timeout(String),
}

/// Result type alias
pub type Result<T> = std::result::Result<T, Error>;
