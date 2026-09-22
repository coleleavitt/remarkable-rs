//! Screen Share Client
//!
//! High-level client that combines MQTT signaling, WebRTC, and RFB decoding.
//! This version uses synchronous Display to avoid Send/Sync issues.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::rfb::RfbDecoder;
use crate::signaling::{SignalingConfig, SignalingEvent};
use crate::webrtc::{ConnectionState, WebRtcEvent, WebRtcHandler};

/// Client errors
#[derive(Error, Debug)]
pub enum ClientError {
    #[error("Signaling error: {0}")]
    Signaling(#[from] crate::signaling::SignalingError),

    #[error("WebRTC error: {0}")]
    WebRtc(#[from] crate::webrtc::WebRtcError),

    #[error("Display error: {0}")]
    Display(#[from] crate::display::DisplayError),

    #[error("Export error: {0}")]
    Export(#[from] crate::export::ExportError),

    #[error("Connection declined")]
    ConnectionDeclined,

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("Disconnected")]
    Disconnected,
}

/// Client state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientState {
    Disconnected,
    Connecting,
    WaitingForOffer,
    Negotiating,
    Connected,
    Streaming,
    Error,
}

/// Screen share client configuration
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// Device token (JWT)
    pub device_token: String,
    /// User token (JWT)
    pub user_token: String,
    /// Enable display window
    pub display: bool,
    /// Output directory for exports
    pub output_dir: Option<String>,
    /// Record GIF
    pub record_gif: bool,
    /// GIF frame delay in ms
    pub gif_frame_delay: u32,
    /// Save PNG snapshots
    pub save_snapshots: bool,
    /// Snapshot interval in seconds
    pub snapshot_interval: u64,
}

impl ClientConfig {
    /// Create config from token files
    pub fn from_files<P: AsRef<Path>>(
        device_token_path: P,
        user_token_path: P,
    ) -> Result<Self, std::io::Error> {
        let device_token = std::fs::read_to_string(device_token_path.as_ref())?.trim().to_string();
        let user_token = std::fs::read_to_string(user_token_path.as_ref())?.trim().to_string();

        Ok(Self {
            device_token,
            user_token,
            display: true,
            output_dir: None,
            record_gif: false,
            gif_frame_delay: 100,
            save_snapshots: false,
            snapshot_interval: 60,
        })
    }

    /// Enable display
    pub fn with_display(mut self, enabled: bool) -> Self {
        self.display = enabled;
        self
    }

    /// Set output directory
    pub fn with_output<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.output_dir = Some(path.as_ref().to_string_lossy().to_string());
        self
    }

    /// Enable GIF recording
    pub fn with_gif(mut self, frame_delay: u32) -> Self {
        self.record_gif = true;
        self.gif_frame_delay = frame_delay;
        self
    }

    /// Enable snapshots
    pub fn with_snapshots(mut self, interval_secs: u64) -> Self {
        self.save_snapshots = true;
        self.snapshot_interval = interval_secs;
        self
    }
}

/// Frame event from WebRTC
pub struct FrameEvent {
    pub data: Vec<u8>,
}

/// Screen share client
pub struct ScreenShareClient {
    config: ClientConfig,
    state: ClientState,
    decoder: RfbDecoder,
}

impl ScreenShareClient {
    /// Create new client
    pub fn new(config: ClientConfig) -> Self {
        Self {
            config,
            state: ClientState::Disconnected,
            decoder: RfbDecoder::new(),
        }
    }

    /// Run the client (blocking on WebRTC events)
    pub async fn run(&mut self) -> Result<(), ClientError> {
        info!("Starting screen share client");

        // Setup signaling
        let signaling_config =
            SignalingConfig::from_tokens(&self.config.device_token, &self.config.user_token)?;

        let mut signaling = crate::signaling::MqttSignaling::new(signaling_config);

        info!("Connecting to MQTT broker...");
        self.state = ClientState::Connecting;
        signaling.connect().await?;

        info!("Subscribing to signaling topics...");
        signaling.subscribe().await?;

        info!("Requesting screen share offer...");
        self.state = ClientState::WaitingForOffer;
        signaling.request_offer().await?;

        // Setup WebRTC
        let (webrtc_tx, mut webrtc_rx) = mpsc::channel::<WebRtcEvent>(32);
        let webrtc = Arc::new(WebRtcHandler::new(webrtc_tx).await?);

        // Wait for offer with timeout
        let offer = tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                match signaling.poll().await {
                    Ok(SignalingEvent::Offer(msg)) => {
                        if let Some(sdp) = msg.sdp {
                            return Ok::<_, ClientError>(sdp);
                        }
                    }
                    Ok(SignalingEvent::Declined) => {
                        return Err(ClientError::ConnectionDeclined);
                    }
                    Ok(SignalingEvent::Error(e)) => {
                        return Err(ClientError::Signaling(
                            crate::signaling::SignalingError::Connection(e),
                        ));
                    }
                    Ok(other) => {
                        debug!(?other, "Signaling event");
                    }
                    Err(e) => {
                        return Err(ClientError::Signaling(e));
                    }
                }
            }
        })
        .await
        .map_err(|_| ClientError::Timeout("waiting for offer".into()))??;

        info!("Received SDP offer, creating answer...");
        self.state = ClientState::Negotiating;

        // Process offer and create answer
        let answer = webrtc.process_offer(&offer).await?;

        // Send answer
        signaling.send_answer(&answer).await?;

        self.state = ClientState::Connected;
        info!("WebRTC connected, waiting for data channel...");

        // Create frame receiver channel
        let (frame_tx, frame_rx) = mpsc::channel::<FrameEvent>(32);

        // Spawn WebRTC event processor
        let _webrtc_clone = Arc::clone(&webrtc);
        let frame_tx_clone = frame_tx;
        tokio::spawn(async move {
            while let Some(event) = webrtc_rx.recv().await {
                match event {
                    WebRtcEvent::Data(data) => {
                        let _ = frame_tx_clone.send(FrameEvent { data }).await;
                    }
                    WebRtcEvent::StateChange(state) => {
                        info!(?state, "WebRTC state changed");
                        if state == ConnectionState::Failed || state == ConnectionState::Closed {
                            break;
                        }
                    }
                    WebRtcEvent::DataChannelOpen => {
                        info!("Data channel opened");
                    }
                    WebRtcEvent::DataChannelClose => {
                        info!("Data channel closed");
                        break;
                    }
                    WebRtcEvent::IceCandidate(_ice) => {
                        // ICE candidates should be sent via signaling
                        // For simplicity, we're ignoring this here
                    }
                }
            }
        });

        // Run frame processing loop (can run display synchronously here)
        self.state = ClientState::Streaming;
        self.process_frames(frame_rx).await?;

        info!("Screen share ended");
        self.state = ClientState::Disconnected;

        Ok(())
    }

    /// Process received frames
    async fn process_frames(&mut self, mut rx: mpsc::Receiver<FrameEvent>) -> Result<(), ClientError> {
        use crate::display::Display;
        use crate::export::GifRecorder;

        // Setup display if enabled
        let mut display = if self.config.display {
            Some(Display::new("reMarkable Screen Share")?)
        } else {
            None
        };

        // Setup GIF recorder if enabled
        let mut recorder = if self.config.record_gif {
            let path = self
                .config
                .output_dir
                .as_ref()
                .map(|d| format!("{}/recording.gif", d))
                .unwrap_or_else(|| "recording.gif".to_string());
            Some(GifRecorder::new(path, self.config.gif_frame_delay))
        } else {
            None
        };

        info!("Processing frames...");

        while let Some(frame) = rx.recv().await {
            // Process RFB message
            if let Err(e) = self.decoder.process_message(&frame.data) {
                warn!(error = %e, "Failed to process RFB message");
                continue;
            }

            let fb = self.decoder.framebuffer();

            // Update display
            if let Some(ref mut disp) = display {
                if disp.should_close() {
                    info!("Window closed");
                    break;
                }
                if let Err(e) = disp.update_grayscale(fb) {
                    warn!(error = %e, "Display update failed");
                }
            }

            // Record to GIF
            if let Some(ref mut rec) = recorder {
                rec.add_frame(fb);
            }
        }

        // Finalize GIF if recording
        if let Some(rec) = recorder {
            info!(frames = rec.frame_count(), "Finalizing GIF...");
            rec.finalize()?;
        }

        Ok(())
    }

    /// Get current state
    pub fn state(&self) -> ClientState {
        self.state
    }

    /// Get current framebuffer
    pub fn framebuffer(&self) -> &[u8] {
        self.decoder.framebuffer()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_config() {
        let config = ClientConfig {
            device_token: "test".into(),
            user_token: "test".into(),
            display: true,
            output_dir: None,
            record_gif: false,
            gif_frame_delay: 100,
            save_snapshots: false,
            snapshot_interval: 60,
        };

        let config = config.with_display(false).with_output("/tmp").with_gif(50);

        assert!(!config.display);
        assert!(config.record_gif);
        assert_eq!(config.gif_frame_delay, 50);
    }
}
