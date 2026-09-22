//! WebRTC Peer Connection Handler for Screen Share
//!
//! Manages WebRTC connection with reMarkable device.
//! Uses DataChannel for RFB framebuffer data transfer.

use std::sync::Arc;

use thiserror::Error;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, info};
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::data_channel::RTCDataChannel;
use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;

/// WebRTC errors
#[derive(Error, Debug)]
pub enum WebRtcError {
    #[error("WebRTC API error: {0}")]
    Api(String),

    #[error("Peer connection error: {0}")]
    PeerConnection(String),

    #[error("SDP error: {0}")]
    Sdp(String),

    #[error("ICE error: {0}")]
    Ice(String),

    #[error("Data channel error: {0}")]
    DataChannel(String),

    #[error("Connection failed")]
    ConnectionFailed,

    #[error("Disconnected")]
    Disconnected,

    #[error("WebRTC error: {0}")]
    WebRtc(#[from] webrtc::Error),
}

/// WebRTC connection state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    New,
    Connecting,
    Connected,
    Disconnected,
    Failed,
    Closed,
}

impl From<RTCPeerConnectionState> for ConnectionState {
    fn from(state: RTCPeerConnectionState) -> Self {
        match state {
            RTCPeerConnectionState::New => Self::New,
            RTCPeerConnectionState::Connecting => Self::Connecting,
            RTCPeerConnectionState::Connected => Self::Connected,
            RTCPeerConnectionState::Disconnected => Self::Disconnected,
            RTCPeerConnectionState::Failed => Self::Failed,
            RTCPeerConnectionState::Closed => Self::Closed,
            _ => Self::New,
        }
    }
}

/// ICE candidate for signaling
#[derive(Debug, Clone)]
pub struct IceCandidate {
    pub candidate: String,
    pub sdp_mid: String,
    pub sdp_mline_index: u32,
}

/// WebRTC event
#[derive(Debug)]
pub enum WebRtcEvent {
    StateChange(ConnectionState),
    IceCandidate(IceCandidate),
    DataChannelOpen,
    DataChannelClose,
    Data(Vec<u8>),
}

/// Default STUN servers
const DEFAULT_STUN_SERVERS: &[&str] = &[
    "stun:stun.l.google.com:19302",
    "stun:stun1.l.google.com:19302",
];

/// WebRTC handler for screen share
pub struct WebRtcHandler {
    peer_connection: Arc<RTCPeerConnection>,
    data_channel: Arc<Mutex<Option<Arc<RTCDataChannel>>>>,
    event_tx: mpsc::Sender<WebRtcEvent>,
    state: Arc<Mutex<ConnectionState>>,
}

impl WebRtcHandler {
    /// Create new WebRTC handler
    pub async fn new(event_tx: mpsc::Sender<WebRtcEvent>) -> Result<Self, WebRtcError> {
        Self::with_stun_servers(DEFAULT_STUN_SERVERS, event_tx).await
    }

    /// Create handler with custom STUN servers
    pub async fn with_stun_servers(
        stun_servers: &[&str],
        event_tx: mpsc::Sender<WebRtcEvent>,
    ) -> Result<Self, WebRtcError> {
        // Create API
        let mut media_engine = MediaEngine::default();
        media_engine.register_default_codecs()?;

        let mut registry = Registry::new();
        registry = register_default_interceptors(registry, &mut media_engine)?;

        let api = APIBuilder::new()
            .with_media_engine(media_engine)
            .with_interceptor_registry(registry)
            .build();

        // ICE servers
        let ice_servers: Vec<RTCIceServer> = stun_servers
            .iter()
            .map(|url| RTCIceServer {
                urls: vec![url.to_string()],
                ..Default::default()
            })
            .collect();

        let config = RTCConfiguration {
            ice_servers,
            ..Default::default()
        };

        let peer_connection = Arc::new(api.new_peer_connection(config).await?);
        let data_channel = Arc::new(Mutex::new(None));
        let state = Arc::new(Mutex::new(ConnectionState::New));

        let handler = Self {
            peer_connection,
            data_channel,
            event_tx,
            state,
        };

        handler.setup_callbacks().await;

        Ok(handler)
    }

    /// Setup event callbacks
    async fn setup_callbacks(&self) {
        let state = Arc::clone(&self.state);
        let tx = self.event_tx.clone();

        // Connection state change
        self.peer_connection
            .on_peer_connection_state_change(Box::new(move |s| {
                let state = Arc::clone(&state);
                let tx = tx.clone();
                Box::pin(async move {
                    let new_state = ConnectionState::from(s);
                    info!(state = ?new_state, "Connection state changed");
                    *state.lock().await = new_state;
                    let _ = tx.send(WebRtcEvent::StateChange(new_state)).await;
                })
            }));

        // ICE candidate
        let tx = self.event_tx.clone();
        self.peer_connection
            .on_ice_candidate(Box::new(move |candidate| {
                let tx = tx.clone();
                Box::pin(async move {
                    if let Some(c) = candidate {
                        if let Ok(json) = c.to_json() {
                            let ice = IceCandidate {
                                candidate: json.candidate,
                                sdp_mid: json.sdp_mid.unwrap_or_default(),
                                sdp_mline_index: json.sdp_mline_index.unwrap_or(0) as u32,
                            };
                            debug!(candidate = %ice.candidate, "Local ICE candidate");
                            let _ = tx.send(WebRtcEvent::IceCandidate(ice)).await;
                        }
                    }
                })
            }));

        // Data channel
        let data_channel = Arc::clone(&self.data_channel);
        let tx = self.event_tx.clone();
        self.peer_connection
            .on_data_channel(Box::new(move |channel| {
                let data_channel = Arc::clone(&data_channel);
                let tx = tx.clone();

                Box::pin(async move {
                    info!(label = %channel.label(), id = channel.id(), "Data channel received");

                    // Store channel
                    *data_channel.lock().await = Some(Arc::clone(&channel));

                    // Open callback
                    let tx_open = tx.clone();
                    channel.on_open(Box::new(move || {
                        let tx = tx_open.clone();
                        Box::pin(async move {
                            info!("Data channel opened");
                            let _ = tx.send(WebRtcEvent::DataChannelOpen).await;
                        })
                    }));

                    // Close callback
                    let tx_close = tx.clone();
                    channel.on_close(Box::new(move || {
                        let tx = tx_close.clone();
                        Box::pin(async move {
                            info!("Data channel closed");
                            let _ = tx.send(WebRtcEvent::DataChannelClose).await;
                        })
                    }));

                    // Message callback
                    let tx_msg = tx.clone();
                    channel.on_message(Box::new(move |msg: DataChannelMessage| {
                        let tx = tx_msg.clone();
                        let data = msg.data.to_vec();
                        Box::pin(async move {
                            debug!(len = data.len(), "Received data");
                            let _ = tx.send(WebRtcEvent::Data(data)).await;
                        })
                    }));
                })
            }));
    }

    /// Get current state
    pub async fn state(&self) -> ConnectionState {
        *self.state.lock().await
    }

    /// Process SDP offer and create answer
    pub async fn process_offer(&self, sdp: &str) -> Result<String, WebRtcError> {
        info!(sdp_len = sdp.len(), "Processing SDP offer");

        let offer = RTCSessionDescription::offer(sdp.to_string())
            .map_err(|e| WebRtcError::Sdp(e.to_string()))?;

        self.peer_connection.set_remote_description(offer).await?;

        let answer = self.peer_connection.create_answer(None).await?;

        self.peer_connection
            .set_local_description(answer.clone())
            .await?;

        info!("Created SDP answer");

        Ok(answer.sdp)
    }

    /// Add remote ICE candidate
    pub async fn add_ice_candidate(
        &self,
        candidate: &str,
        sdp_mid: &str,
        sdp_mline_index: u32,
    ) -> Result<(), WebRtcError> {
        debug!(candidate = %candidate, "Adding ICE candidate");

        let init = RTCIceCandidateInit {
            candidate: candidate.to_string(),
            sdp_mid: Some(sdp_mid.to_string()),
            sdp_mline_index: Some(sdp_mline_index as u16),
            username_fragment: None,
        };

        self.peer_connection.add_ice_candidate(init).await?;

        Ok(())
    }

    /// Send data through data channel
    pub async fn send(&self, data: &[u8]) -> Result<(), WebRtcError> {
        let channel = self.data_channel.lock().await;
        if let Some(ch) = channel.as_ref() {
            ch.send(&bytes::Bytes::copy_from_slice(data))
                .await
                .map_err(|e| WebRtcError::DataChannel(e.to_string()))?;
            Ok(())
        } else {
            Err(WebRtcError::DataChannel("No data channel".into()))
        }
    }

    /// Close connection
    pub async fn close(&self) -> Result<(), WebRtcError> {
        self.peer_connection.close().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_state_from() {
        assert_eq!(
            ConnectionState::from(RTCPeerConnectionState::Connected),
            ConnectionState::Connected
        );
        assert_eq!(
            ConnectionState::from(RTCPeerConnectionState::Failed),
            ConnectionState::Failed
        );
    }
}
