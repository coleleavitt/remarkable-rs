//! WebRTC Handler for Screen Share
//!
//! Manages WebRTC peer connections and data channels using webrtc-rs.

use std::sync::Arc;

use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, trace, warn};
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::setting_engine::SettingEngine;
use webrtc::api::APIBuilder;
use webrtc::ice::udp_network::{EphemeralUDP, UDPNetwork};
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::data_channel::RTCDataChannel;
use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;

use crate::error::{Error, Result};
use crate::rfb::{CHANNEL_LABEL, CLIENT_HANDSHAKE};

/// WebRTC connection state
#[derive(Debug, Clone, Copy, PartialEq)]
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
            RTCPeerConnectionState::Unspecified => Self::New,
        }
    }
}

/// ICE candidate for signaling
#[derive(Debug, Clone)]
pub struct IceCandidate {
    pub candidate: String,
    pub sdp_mid: Option<String>,
    pub sdp_mline_index: Option<u16>,
}

/// WebRTC handler
pub struct WebRtcHandler {
    peer_connection: Arc<RTCPeerConnection>,
    data_channel: Arc<RwLock<Option<Arc<RTCDataChannel>>>>,
    state: Arc<RwLock<ConnectionState>>,
}

/// A STUN or TURN server.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IceServer {
    pub urls: Vec<String>,
    /// TURN only.
    pub username: String,
    /// TURN only.
    pub credential: String,
}

impl IceServer {
    /// A server without credentials, e.g. `stun:stun.l.google.com:19302`.
    pub fn url(url: impl Into<String>) -> Self {
        Self { urls: vec![url.into()], ..Self::default() }
    }
}

/// How the local end of the peer connection is set up.
#[derive(Debug, Clone, Default)]
pub struct TransportConfig {
    /// STUN/TURN servers. Empty means host candidates only, which is enough
    /// when this end has a reachable address (same LAN, or a public server).
    pub ice_servers: Vec<IceServer>,
    /// Restrict ICE to this UDP port range, e.g. to match a firewall rule.
    pub udp_ports: Option<(u16, u16)>,
}

impl WebRtcHandler {
    /// Create the answering side of a screen share peer connection.
    ///
    /// Returns the handler, local ICE candidates to signal to the tablet, and
    /// the raw bytes of the tablet's `screenshare` data channel.
    pub async fn new(
        config: TransportConfig,
    ) -> Result<(Self, mpsc::Receiver<IceCandidate>, mpsc::UnboundedReceiver<Vec<u8>>)> {
        // Set up media engine (not used for data-only, but required)
        let mut media_engine = MediaEngine::default();
        
        // Set up interceptors
        let mut registry = Registry::new();
        registry = register_default_interceptors(registry, &mut media_engine)
            .map_err(|e| Error::WebRtc(format!("Failed to register interceptors: {}", e)))?;
        
        // A full-screen frame is well over the 64 KiB default. This value is
        // advertised to the tablet (`a=max-message-size`) and sizes both the
        // data-channel read buffer and the SCTP receive window (webrtc-rs#908).
        let mut setting_engine = SettingEngine::default();
        setting_engine.set_sctp_max_message_size_can_receive(MAX_MESSAGE_SIZE);
        if let Some((min, max)) = config.udp_ports {
            let range = EphemeralUDP::new(min, max)
                .map_err(|e| Error::WebRtc(format!("Invalid UDP port range {min}-{max}: {e}")))?;
            setting_engine.set_udp_network(UDPNetwork::Ephemeral(range));
        }

        let api = APIBuilder::new()
            .with_media_engine(media_engine)
            .with_interceptor_registry(registry)
            .with_setting_engine(setting_engine)
            .build();
        
        let ice_servers: Vec<RTCIceServer> = config
            .ice_servers
            .iter()
            .map(|s| RTCIceServer {
                urls: s.urls.clone(),
                username: s.username.clone(),
                credential: s.credential.clone(),
            })
            .collect();
        
        let config = RTCConfiguration {
            ice_servers,
            ..Default::default()
        };
        
        // Create peer connection
        let peer_connection = api
            .new_peer_connection(config)
            .await
            .map_err(|e| Error::PeerConnection(format!("Failed to create peer connection: {}", e)))?;
        
        let peer_connection = Arc::new(peer_connection);
        let state = Arc::new(RwLock::new(ConnectionState::New));
        let data_channel: Arc<RwLock<Option<Arc<RTCDataChannel>>>> = Arc::new(RwLock::new(None));
        
        // Create channels
        let (ice_tx, ice_rx) = mpsc::channel(32);
        // Unbounded and lossless: the protocol is a byte stream, so dropping a
        // chunk would corrupt every message after it.
        let (data_tx, data_rx) = mpsc::unbounded_channel();
        let data_tx = Arc::new(std::sync::Mutex::new(Some(data_tx)));
        
        // Set up state change handler
        let state_clone = state.clone();
        let state_data_tx = data_tx.clone();
        peer_connection.on_peer_connection_state_change(Box::new(move |s| {
            let state = state_clone.clone();
            // The tablet tears the peer down on Disconnected too (libdatachannel
            // webrtc.cpp), so the session is over either way.
            if matches!(s, RTCPeerConnectionState::Disconnected | RTCPeerConnectionState::Failed | RTCPeerConnectionState::Closed) {
                state_data_tx.lock().unwrap().take();
            }
            Box::pin(async move {
                info!("Peer connection state changed: {:?}", s);
                *state.write().await = ConnectionState::from(s);
            })
        }));
        
        // Set up ICE candidate handler
        let ice_tx_clone = ice_tx.clone();
        peer_connection.on_ice_candidate(Box::new(move |candidate| {
            let ice_tx = ice_tx_clone.clone();
            Box::pin(async move {
                if let Some(c) = candidate {
                    debug!("New ICE candidate: {}", c.to_json().unwrap_or_default().candidate);
                    if let Ok(json) = c.to_json() {
                        let _ = ice_tx.send(IceCandidate {
                            candidate: json.candidate,
                            sdp_mid: json.sdp_mid,
                            sdp_mline_index: json.sdp_mline_index,
                        }).await;
                    }
                }
            })
        }));
        
        // Set up data channel handler. The tablet opens the channel; we answer
        // with the protocol handshake and then only listen (see `rfb`).
        let data_channel_clone = data_channel.clone();
        let data_tx_clone = data_tx.clone();
        peer_connection.on_data_channel(Box::new(move |dc: Arc<RTCDataChannel>| {
            let data_channel = data_channel_clone.clone();
            let data_tx = data_tx_clone.clone();

            info!("Data channel opened: {}", dc.label());
            if dc.label() != CHANNEL_LABEL {
                warn!("Unexpected data channel label {:?} (expected {:?})", dc.label(), CHANNEL_LABEL);
            }

            Box::pin(async move {
                *data_channel.write().await = Some(Arc::clone(&dc));

                let dc_open = Arc::clone(&dc);
                dc.on_open(Box::new(move || {
                    let dc = Arc::clone(&dc_open);
                    Box::pin(async move {
                        match dc.send(&bytes::Bytes::from_static(CLIENT_HANDSHAKE)).await {
                            Ok(_) => info!("Sent client handshake ({} bytes)", CLIENT_HANDSHAKE.len()),
                            Err(e) => error!("Failed to send handshake: {}", e),
                        }
                    })
                }));

                let msg_tx = data_tx.clone();
                dc.on_message(Box::new(move |msg: DataChannelMessage| {
                    debug!("Data channel message: {} bytes", msg.data.len());
                    trace!("Data channel message head: {:02x?}", &msg.data[..msg.data.len().min(48)]);
                    if let Some(tx) = msg_tx.lock().unwrap().as_ref() {
                        let _ = tx.send(msg.data.to_vec());
                    }
                    Box::pin(async {})
                }));

                dc.on_error(Box::new(|e| {
                    error!("Data channel error: {}", e);
                    Box::pin(async {})
                }));

                dc.on_close(Box::new(move || {
                    info!("Data channel closed");
                    // Dropping the sender ends the receiver's stream.
                    data_tx.lock().unwrap().take();
                    Box::pin(async {})
                }));
            })
        }));

        Ok((
            Self {
                peer_connection,
                data_channel,
                state,
            },
            ice_rx,
            data_rx,
        ))
    }
    
    /// Get current connection state
    pub async fn state(&self) -> ConnectionState {
        *self.state.read().await
    }
    
    /// Set remote offer and create answer
    pub async fn accept_offer(&self, sdp: &str) -> Result<String> {
        let offer = RTCSessionDescription::offer(sdp.to_string())
            .map_err(|e| Error::WebRtc(format!("Invalid SDP offer: {}", e)))?;
        
        self.peer_connection
            .set_remote_description(offer)
            .await
            .map_err(|e| Error::WebRtc(format!("Failed to set remote description: {}", e)))?;
        
        let answer = self.peer_connection
            .create_answer(None)
            .await
            .map_err(|e| Error::WebRtc(format!("Failed to create answer: {}", e)))?;
        
        let sdp = answer.sdp.clone();
        
        self.peer_connection
            .set_local_description(answer)
            .await
            .map_err(|e| Error::WebRtc(format!("Failed to set local description: {}", e)))?;
        
        Ok(sdp)
    }
    
    /// Add remote ICE candidate
    pub async fn add_ice_candidate(
        &self,
        candidate: &str,
        sdp_mid: Option<&str>,
        sdp_mline_index: Option<u16>,
    ) -> Result<()> {
        let init = RTCIceCandidateInit {
            candidate: candidate.to_string(),
            sdp_mid: sdp_mid.map(String::from),
            sdp_mline_index,
            ..Default::default()
        };
        
        self.peer_connection
            .add_ice_candidate(init)
            .await
            .map_err(|e| Error::IceNegotiation(format!("Failed to add ICE candidate: {}", e)))?;
        
        Ok(())
    }
    
    /// Send data on the data channel
    pub async fn send(&self, data: &[u8]) -> Result<()> {
        let dc = self.data_channel.read().await;
        let dc = dc.as_ref()
            .ok_or_else(|| Error::WebRtc("Data channel not open".into()))?;
        
        dc.send(&bytes::Bytes::copy_from_slice(data))
            .await
            .map_err(|e| Error::WebRtc(format!("Failed to send data: {}", e)))?;
        
        Ok(())
    }
    
    /// Close the peer connection
    pub async fn close(&self) -> Result<()> {
        self.peer_connection
            .close()
            .await
            .map_err(|e| Error::WebRtc(format!("Failed to close connection: {}", e)))?;
        
        Ok(())
    }
}

/// Largest data-channel message we accept. A raw 1404x1872 RGB565 frame is
/// about 5.3 MB and compressed updates are far smaller.
const MAX_MESSAGE_SIZE: u32 = 8 * 1024 * 1024;
