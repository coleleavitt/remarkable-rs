//! Screen Share Viewer
//!
//! Combines MQTT signaling, WebRTC, and RFB decoding into a unified viewer.

use std::sync::Arc;

use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::mqtt::{MqttConfig, MqttSignaling, SignalingMessage};
use crate::rfb::{Event, RfbDecoder, PING_TIMEOUT};
use crate::token::TokenPair;
use crate::cloud::CloudConfig;
use crate::usb::{Frame, UsbCapture, UsbConfig};
use crate::webrtc::WebRtcHandler;

/// Viewer mode
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ViewerMode {
    /// WebRTC via cloud (requires tokens and network)
    WebRtc,
    /// Direct USB framebuffer capture (requires SSH access)
    Usb,
    /// WebRTC via a self-hosted remarkable-server broker over the internet
    Cloud,
}

/// Viewer state
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ViewerState {
    Disconnected,
    Connecting,
    Connected,
    Streaming,
    Error,
}

/// Screen share viewer configuration
#[derive(Debug, Clone)]
pub struct ViewerConfig {
    pub mode: ViewerMode,
    pub tokens: Option<TokenPair>,
    pub mqtt_config: Option<MqttConfig>,
    pub usb_config: Option<UsbConfig>,
    pub stun_servers: Option<Vec<String>>,
    pub cloud_config: Option<CloudConfig>,
}

impl Default for ViewerConfig {
    fn default() -> Self {
        Self {
            mode: ViewerMode::Usb,
            tokens: None,
            mqtt_config: None,
            usb_config: Some(UsbConfig::default()),
            stun_servers: None,
            cloud_config: None,
        }
    }
}

impl ViewerConfig {
    /// Create WebRTC viewer config
    pub fn webrtc(tokens: TokenPair) -> Self {
        Self {
            mode: ViewerMode::WebRtc,
            tokens: Some(tokens),
            mqtt_config: Some(MqttConfig::default()),
            usb_config: None,
            stun_servers: None,
            cloud_config: None,
        }
    }
    
    /// Create cloud (self-hosted broker) viewer config
    pub fn cloud(cfg: CloudConfig) -> Self {
        Self {
            mode: ViewerMode::Cloud,
            tokens: None,
            mqtt_config: None,
            usb_config: None,
            stun_servers: None,
            cloud_config: Some(cfg),
        }
    }

    /// Create USB viewer config
    pub fn usb() -> Self {
        Self {
            mode: ViewerMode::Usb,
            tokens: None,
            mqtt_config: None,
            usb_config: Some(UsbConfig::default()),
            stun_servers: None,
            cloud_config: None,
        }
    }
}

/// Screen share viewer
pub struct ScreenShareViewer {
    config: ViewerConfig,
    state: Arc<RwLock<ViewerState>>,
    frame_tx: broadcast::Sender<Frame>,
}

impl ScreenShareViewer {
    /// Create new viewer
    pub fn new(config: ViewerConfig) -> Self {
        let (frame_tx, _) = broadcast::channel(4);
        Self {
            config,
            state: Arc::new(RwLock::new(ViewerState::Disconnected)),
            frame_tx,
        }
    }
    
    /// Get current state
    pub async fn state(&self) -> ViewerState {
        *self.state.read().await
    }
    
    /// Subscribe to frame updates
    pub fn subscribe(&self) -> broadcast::Receiver<Frame> {
        self.frame_tx.subscribe()
    }
    
    /// Start the viewer
    pub async fn start(&self) -> Result<()> {
        match self.config.mode {
            ViewerMode::WebRtc => self.start_webrtc().await,
            ViewerMode::Usb => self.start_usb().await,
            ViewerMode::Cloud => self.start_cloud().await,
        }
    }
    
    /// Start WebRTC viewer
    async fn start_webrtc(&self) -> Result<()> {
        let tokens = self.config.tokens.clone()
            .ok_or_else(|| Error::Config("Tokens required for WebRTC mode".into()))?;
        
        let mqtt_config = self.config.mqtt_config.clone()
            .unwrap_or_default();
        
        *self.state.write().await = ViewerState::Connecting;
        info!("Starting WebRTC viewer...");
        
        // Create signaling client
        let signaling = MqttSignaling::new(tokens.clone(), mqtt_config)?;
        let mut signal_rx = signaling.subscribe();
        
        // Connect to MQTT
        signaling.connect().await?;
        info!("Connected to MQTT broker");
        
        // Create WebRTC handler
        let (webrtc, mut ice_rx, mut data_rx) = 
            WebRtcHandler::new(self.config.stun_servers.clone()).await?;
        let webrtc = Arc::new(webrtc);
        
        // Generate peer ID
        let peer_id = Uuid::new_v4().to_string();
        
        // Request offer from device
        signaling.request_offer(&peer_id).await?;
        info!("Requested offer from device");
        
        // Handle signaling messages
        let webrtc_clone = webrtc.clone();
        let signaling = Arc::new(signaling);
        let signaling_clone = signaling.clone();
        let _peer_id_clone = peer_id.clone();
        
        tokio::spawn(async move {
            while let Ok(msg) = signal_rx.recv().await {
                match msg {
                    SignalingMessage::Offer { id, sdp } => {
                        info!("Received offer from device");
                        match webrtc_clone.accept_offer(&sdp).await {
                            Ok(answer) => {
                                if let Err(e) = signaling_clone.send_answer(&id, &answer).await {
                                    error!("Failed to send answer: {}", e);
                                }
                            }
                            Err(e) => {
                                error!("Failed to accept offer: {}", e);
                            }
                        }
                    }
                    SignalingMessage::Candidate { candidate, sdp_mid, sdp_mline_index, .. } => {
                        debug!("Received ICE candidate");
                        if let Err(e) = webrtc_clone.add_ice_candidate(
                            &candidate,
                            sdp_mid.as_deref(),
                            sdp_mline_index.map(|x| x as u16),
                        ).await {
                            warn!("Failed to add ICE candidate: {}", e);
                        }
                    }
                    SignalingMessage::ConnectionDeclined { .. } => {
                        error!("Connection declined by device");
                        break;
                    }
                    SignalingMessage::ScreenShareDeclined { .. } => {
                        error!("Screen share declined by device");
                        break;
                    }
                    _ => {}
                }
            }
        });
        
        // Forward local ICE candidates
        let signaling_clone = signaling.clone();
        let peer_id_clone2 = peer_id.clone();
        tokio::spawn(async move {
            while let Some(candidate) = ice_rx.recv().await {
                if let Err(e) = signaling_clone.send_candidate(
                    &peer_id_clone2,
                    &candidate.candidate,
                    candidate.sdp_mid.as_deref(),
                    candidate.sdp_mline_index.map(|x| x as u32),
                ).await {
                    warn!("Failed to send ICE candidate: {}", e);
                }
            }
        });
        
        let frame_tx = self.frame_tx.clone();
        let state = self.state.clone();
        tokio::spawn(async move {
            *state.write().await = ViewerState::Streaming;
            if let Err(e) = pump_frames(&mut data_rx, &frame_tx).await {
                error!("Screen share stream ended: {}", e);
            }
            drop(webrtc);
            *state.write().await = ViewerState::Disconnected;
        });

        *self.state.write().await = ViewerState::Connected;
        Ok(())
    }
    
    /// Start cloud viewer: signaling through a self-hosted broker, then WebRTC.
    async fn start_cloud(&self) -> Result<()> {
        let cfg = self.config.cloud_config.clone()
            .ok_or_else(|| Error::Config("Cloud mode requires cloud_config".into()))?;
        *self.state.write().await = ViewerState::Connecting;
        let session = match crate::cloud::connect(cfg).await {
            Ok(s) => s,
            Err(e) => {
                error!("Cloud connect failed: {}", e);
                *self.state.write().await = ViewerState::Error;
                return Err(e);
            }
        };

        let frame_tx = self.frame_tx.clone();
        let state = self.state.clone();
        tokio::spawn(async move {
            // Own the session here so the peer connection and signaling task live
            // exactly as long as frames are flowing.
            let crate::cloud::CloudSession { webrtc, mut data_rx, signaling_task } = session;
            *state.write().await = ViewerState::Streaming;
            if let Err(e) = pump_frames(&mut data_rx, &frame_tx).await {
                error!("Screen share stream ended: {}", e);
            }
            signaling_task.abort();
            let _ = webrtc.close().await;
            *state.write().await = ViewerState::Disconnected;
        });

        *self.state.write().await = ViewerState::Connected;
        Ok(())
    }

    /// Start USB viewer
    async fn start_usb(&self) -> Result<()> {
        let usb_config = self.config.usb_config.clone()
            .unwrap_or_default();
        
        *self.state.write().await = ViewerState::Connecting;
        info!("Starting USB viewer...");
        
        let capture = UsbCapture::with_config(usb_config.clone());
        
        // Test connection
        if !capture.test_connection().await? {
            return Err(Error::UsbConnection("Cannot connect to device".into()));
        }
        
        // Get device info
        let info = capture.get_device_info().await?;
        info!("Connected to {} running firmware {}", info.model, info.firmware_version);
        
        // Start continuous capture
        let mut frame_rx = capture.start_continuous(10).await?;
        
        let frame_tx = self.frame_tx.clone();
        let state = self.state.clone();
        
        tokio::spawn(async move {
            *state.write().await = ViewerState::Streaming;
            
            while let Some(frame) = frame_rx.recv().await {
                let _ = frame_tx.send(frame);
            }
            
            *state.write().await = ViewerState::Disconnected;
        });
        
        *self.state.write().await = ViewerState::Connected;
        Ok(())
    }
    
    /// Get a single frame
    pub async fn get_frame(&self) -> Result<Frame> {
        match self.config.mode {
            ViewerMode::Usb => {
                let usb_config = self.config.usb_config.clone()
                    .unwrap_or_default();
                let capture = UsbCapture::with_config(usb_config);
                capture.capture_frame().await
            }
            ViewerMode::WebRtc | ViewerMode::Cloud => {
                // For WebRTC, we need to wait for a frame
                let mut rx = self.subscribe();
                rx.recv().await
                    .map_err(|_| Error::Timeout("No frame received".into()))
            }
        }
    }
}

/// Decode the tablet's byte stream and publish a frame whenever the picture
/// changes. Returns when the channel closes, the tablet shuts down, pings
/// stop, or the stream is malformed.
async fn pump_frames(
    data_rx: &mut mpsc::UnboundedReceiver<Vec<u8>>,
    frame_tx: &broadcast::Sender<Frame>,
) -> Result<()> {
    let mut decoder = RfbDecoder::new();
    loop {
        // The tablet pings every few seconds; silence means it is gone.
        let data = match tokio::time::timeout(PING_TIMEOUT, data_rx.recv()).await {
            Ok(Some(data)) => data,
            Ok(None) => return Err(Error::WebRtc("data channel closed".into())),
            Err(_) => return Err(Error::Timeout(format!("no data from tablet for {:?}", PING_TIMEOUT))),
        };
        let mut changed = false;
        for event in decoder.feed(&data)? {
            match event {
                Event::Handshake { version, width, height } => {
                    info!("Screen share handshake: server v{} {}x{}", version, width, height);
                }
                Event::FramebufferUpdated { dirty, rects } => {
                    debug!("Framebuffer update: {} rects, dirty {:?}", rects, dirty);
                    changed = true;
                }
                Event::Rotation(degrees) => {
                    info!("Tablet rotation: {}°", degrees);
                    changed = decoder.is_ready();
                }
                Event::Cursor { x, y } => debug!("Cursor at {},{}", x, y),
                Event::Ping => debug!("Ping"),
                Event::Shutdown => {
                    info!("Tablet stopped screen share");
                    return Ok(());
                }
            }
        }
        // One frame per data-channel message, however many updates it held.
        if changed {
            let (data, width, height) = decoder.gray_frame();
            let _ = frame_tx.send(Frame { data, width, height, timestamp: std::time::Instant::now() });
        }
    }
}
