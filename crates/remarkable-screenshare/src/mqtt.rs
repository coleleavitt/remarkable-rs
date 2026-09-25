//! MQTT Signaling for WebRTC
//!
//! Handles MQTT-based signaling for WebRTC connection establishment
//! with the reMarkable cloud infrastructure.

use std::sync::Arc;
use std::time::Duration;

use rumqttc::{AsyncClient, Event, EventLoop, MqttOptions, Packet, QoS, TlsConfiguration, Transport};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc, Mutex, RwLock};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::constants::{
    msg_type, MQTT_BROKER_PROD, MQTT_BROKER_WS_PROD, MQTT_PORT_TLS, MQTT_PORT_WS, MQTT_WS_PATH,
    TOPIC_SIGNALING_USER, TOPIC_SUBSCRIBE_PATTERN,
};
use crate::error::{Error, Result};
use crate::token::TokenPair;

/// Signaling state
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SignalingState {
    Disconnected,
    Connecting,
    Connected,
    WaitingForOffer,
    Negotiating,
    Established,
    Error,
}

/// Signaling message types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "kebab-case")]
pub enum SignalingMessage {
    RequestOffer {
        id: String,
    },
    Offer {
        id: String,
        sdp: String,
    },
    Answer {
        id: String,
        sdp: String,
    },
    Candidate {
        id: String,
        candidate: String,
        #[serde(rename = "sdpMid")]
        sdp_mid: Option<String>,
        #[serde(rename = "sdpMLineIndex")]
        sdp_mline_index: Option<u32>,
    },
    RoomCreated {
        id: String,
        #[serde(rename = "roomId")]
        room_id: String,
    },
    ConnectionDeclined {
        id: String,
    },
    ScreenShareDeclined {
        id: String,
    },
}

impl SignalingMessage {
    pub fn request_offer(id: &str) -> Self {
        Self::RequestOffer { id: id.to_string() }
    }
    
    pub fn answer(id: &str, sdp: &str) -> Self {
        Self::Answer {
            id: id.to_string(),
            sdp: sdp.to_string(),
        }
    }
    
    pub fn candidate(id: &str, candidate: &str, sdp_mid: Option<&str>, sdp_mline_index: Option<u32>) -> Self {
        Self::Candidate {
            id: id.to_string(),
            candidate: candidate.to_string(),
            sdp_mid: sdp_mid.map(String::from),
            sdp_mline_index,
        }
    }
    
    /// Get message type as string
    pub fn msg_type(&self) -> &str {
        match self {
            Self::RequestOffer { .. } => msg_type::REQUEST_OFFER,
            Self::Offer { .. } => msg_type::OFFER,
            Self::Answer { .. } => msg_type::ANSWER,
            Self::Candidate { .. } => msg_type::CANDIDATE,
            Self::RoomCreated { .. } => msg_type::ROOM_CREATED,
            Self::ConnectionDeclined { .. } => msg_type::CONNECTION_DECLINED,
            Self::ScreenShareDeclined { .. } => msg_type::SCREENSHARE_DECLINED,
        }
    }
    
    /// Get peer ID from message
    pub fn id(&self) -> &str {
        match self {
            Self::RequestOffer { id } => id,
            Self::Offer { id, .. } => id,
            Self::Answer { id, .. } => id,
            Self::Candidate { id, .. } => id,
            Self::RoomCreated { id, .. } => id,
            Self::ConnectionDeclined { id } => id,
            Self::ScreenShareDeclined { id } => id,
        }
    }
}

/// MQTT Signaling configuration
#[derive(Debug, Clone)]
pub struct MqttConfig {
    pub broker_host: String,
    pub port: u16,
    pub use_websocket: bool,
    pub client_id: String,
}

impl Default for MqttConfig {
    fn default() -> Self {
        Self {
            broker_host: MQTT_BROKER_PROD.to_string(),
            port: MQTT_PORT_TLS,
            use_websocket: false,
            client_id: format!("screenshare-{}", Uuid::new_v4()),
        }
    }
}

impl MqttConfig {
    /// Create WebSocket configuration
    pub fn websocket() -> Self {
        Self {
            broker_host: MQTT_BROKER_WS_PROD.to_string(),
            port: MQTT_PORT_WS,
            use_websocket: true,
            client_id: format!("screenshare-{}", Uuid::new_v4()),
        }
    }
}

/// MQTT Signaling client
pub struct MqttSignaling {
    config: MqttConfig,
    tokens: TokenPair,
    user_id: String,
    state: Arc<RwLock<SignalingState>>,
    client: Arc<Mutex<Option<AsyncClient>>>,
    message_tx: broadcast::Sender<SignalingMessage>,
}

impl MqttSignaling {
    /// Create new signaling client
    pub fn new(tokens: TokenPair, config: MqttConfig) -> Result<Self> {
        let user_id = tokens.user_id()?;
        let (message_tx, _) = broadcast::channel(32);
        
        Ok(Self {
            config,
            tokens,
            user_id,
            state: Arc::new(RwLock::new(SignalingState::Disconnected)),
            client: Arc::new(Mutex::new(None)),
            message_tx,
        })
    }
    
    /// Get current state
    pub async fn state(&self) -> SignalingState {
        *self.state.read().await
    }
    
    /// Subscribe to incoming messages
    pub fn subscribe(&self) -> broadcast::Receiver<SignalingMessage> {
        self.message_tx.subscribe()
    }
    
    /// Connect to MQTT broker
    pub async fn connect(&self) -> Result<()> {
        *self.state.write().await = SignalingState::Connecting;
        
        let mut mqtt_options = MqttOptions::new(
            &self.config.client_id,
            &self.config.broker_host,
            self.config.port,
        );
        
        // Set credentials from tokens
        mqtt_options.set_credentials(&self.tokens.device_token, &self.tokens.user_token);
        mqtt_options.set_keep_alive(Duration::from_secs(30));
        
        // Configure TLS
        let tls_config = TlsConfiguration::default();
        
        if self.config.use_websocket {
            let url = format!(
                "wss://{}:{}{}", 
                self.config.broker_host, 
                self.config.port,
                MQTT_WS_PATH
            );
            mqtt_options.set_transport(Transport::wss_with_default_config());
        } else {
            mqtt_options.set_transport(Transport::tls_with_default_config());
        }
        
        let (client, eventloop) = AsyncClient::new(mqtt_options, 10);
        *self.client.lock().await = Some(client.clone());
        
        // Start event loop in background
        let state = self.state.clone();
        let message_tx = self.message_tx.clone();
        let user_id = self.user_id.clone();
        
        tokio::spawn(async move {
            Self::run_event_loop(eventloop, state, message_tx, user_id).await;
        });
        
        // Wait for connection
        tokio::time::sleep(Duration::from_millis(500)).await;
        
        if *self.state.read().await == SignalingState::Connecting {
            *self.state.write().await = SignalingState::Connected;
        }
        
        // Subscribe to signaling topic
        self.subscribe_to_topics().await?;
        
        Ok(())
    }
    
    /// Subscribe to signaling topics
    async fn subscribe_to_topics(&self) -> Result<()> {
        let client = self.client.lock().await;
        let client = client.as_ref()
            .ok_or_else(|| Error::MqttConnection("Not connected".into()))?;
        
        let topic = format!("user/{}/signaling/#", self.user_id);
        client.subscribe(&topic, QoS::AtLeastOnce).await
            .map_err(|e| Error::MqttSignaling(format!("Subscribe failed: {}", e)))?;
        
        debug!("Subscribed to: {}", topic);
        Ok(())
    }
    
    /// Send signaling message
    pub async fn send(&self, message: &SignalingMessage) -> Result<()> {
        let client = self.client.lock().await;
        let client = client.as_ref()
            .ok_or_else(|| Error::MqttConnection("Not connected".into()))?;
        
        let topic = format!("remarkable/screenshare/signaling/user/{}", self.user_id);
        let payload = serde_json::to_vec(message)?;
        
        client.publish(&topic, QoS::AtLeastOnce, false, payload).await
            .map_err(|e| Error::MqttSignaling(format!("Publish failed: {}", e)))?;
        
        debug!("Sent {} to {}", message.msg_type(), topic);
        Ok(())
    }
    
    /// Request offer from device
    pub async fn request_offer(&self, peer_id: &str) -> Result<()> {
        *self.state.write().await = SignalingState::WaitingForOffer;
        let msg = SignalingMessage::request_offer(peer_id);
        self.send(&msg).await
    }
    
    /// Send answer
    pub async fn send_answer(&self, peer_id: &str, sdp: &str) -> Result<()> {
        *self.state.write().await = SignalingState::Negotiating;
        let msg = SignalingMessage::answer(peer_id, sdp);
        self.send(&msg).await
    }
    
    /// Send ICE candidate
    pub async fn send_candidate(
        &self,
        peer_id: &str,
        candidate: &str,
        sdp_mid: Option<&str>,
        sdp_mline_index: Option<u32>,
    ) -> Result<()> {
        let msg = SignalingMessage::candidate(peer_id, candidate, sdp_mid, sdp_mline_index);
        self.send(&msg).await
    }
    
    /// Disconnect
    pub async fn disconnect(&self) -> Result<()> {
        if let Some(client) = self.client.lock().await.take() {
            client.disconnect().await
                .map_err(|e| Error::MqttConnection(format!("Disconnect failed: {}", e)))?;
        }
        *self.state.write().await = SignalingState::Disconnected;
        Ok(())
    }
    
    /// Run MQTT event loop
    async fn run_event_loop(
        mut eventloop: EventLoop,
        state: Arc<RwLock<SignalingState>>,
        message_tx: broadcast::Sender<SignalingMessage>,
        _user_id: String,
    ) {
        loop {
            match eventloop.poll().await {
                Ok(Event::Incoming(Packet::Publish(publish))) => {
                    let topic = publish.topic.as_str();
                    debug!("Received message on topic: {}", topic);
                    
                    // Parse message
                    match serde_json::from_slice::<SignalingMessage>(&publish.payload) {
                        Ok(msg) => {
                            info!("Received {} from peer {}", msg.msg_type(), msg.id());
                            
                            // Update state based on message type
                            match &msg {
                                SignalingMessage::Offer { .. } => {
                                    *state.write().await = SignalingState::Negotiating;
                                }
                                SignalingMessage::ConnectionDeclined { .. } => {
                                    *state.write().await = SignalingState::Error;
                                    warn!("Connection declined by device");
                                }
                                SignalingMessage::ScreenShareDeclined { .. } => {
                                    *state.write().await = SignalingState::Error;
                                    warn!("Screen share declined by device");
                                }
                                _ => {}
                            }
                            
                            let _ = message_tx.send(msg);
                        }
                        Err(e) => {
                            warn!("Failed to parse signaling message: {}", e);
                        }
                    }
                }
                Ok(Event::Incoming(Packet::ConnAck(_))) => {
                    info!("Connected to MQTT broker");
                    *state.write().await = SignalingState::Connected;
                }
                Ok(Event::Incoming(Packet::Disconnect)) => {
                    warn!("Disconnected from MQTT broker");
                    *state.write().await = SignalingState::Disconnected;
                    break;
                }
                Err(e) => {
                    error!("MQTT error: {}", e);
                    *state.write().await = SignalingState::Error;
                    break;
                }
                _ => {}
            }
        }
    }
}
