//! Screen share viewer: frames from the tablet over the cloud broker, or from
//! its framebuffer over USB.

use std::sync::Arc;

use tokio::sync::{broadcast, RwLock};
use tracing::{error, info};

use crate::cloud::CloudConfig;
use crate::error::{Error, Result};
use crate::session::{pump_frames, Frame};
use crate::usb::{UsbCapture, UsbConfig};

/// Where frames come from.
#[derive(Debug, Clone)]
pub enum ViewerSource {
    /// WebRTC negotiated through a remarkable-server broker.
    Cloud(CloudConfig),
    /// Framebuffer capture over SSH (USB network).
    Usb(UsbConfig),
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

/// Screen share viewer
pub struct ScreenShareViewer {
    source: ViewerSource,
    state: Arc<RwLock<ViewerState>>,
    frame_tx: broadcast::Sender<Frame>,
}

impl ScreenShareViewer {
    pub fn new(source: ViewerSource) -> Self {
        let (frame_tx, _) = broadcast::channel(4);
        Self { source, state: Arc::new(RwLock::new(ViewerState::Disconnected)), frame_tx }
    }

    pub async fn state(&self) -> ViewerState {
        *self.state.read().await
    }

    /// Subscribe to frame updates
    pub fn subscribe(&self) -> broadcast::Receiver<Frame> {
        self.frame_tx.subscribe()
    }

    /// Start producing frames in the background.
    pub async fn start(&self) -> Result<()> {
        match &self.source {
            ViewerSource::Cloud(cfg) => self.start_cloud(cfg.clone()).await,
            ViewerSource::Usb(cfg) => self.start_usb(cfg.clone()).await,
        }
    }

    async fn start_cloud(&self, cfg: CloudConfig) -> Result<()> {
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
            if let Err(e) = pump_frames(&mut data_rx, |frame| {
                let _ = frame_tx.send(frame);
            })
            .await
            {
                error!("Screen share stream ended: {}", e);
            }
            signaling_task.abort();
            let _ = webrtc.close().await;
            *state.write().await = ViewerState::Disconnected;
        });

        *self.state.write().await = ViewerState::Connected;
        Ok(())
    }

    async fn start_usb(&self, usb_config: UsbConfig) -> Result<()> {
        *self.state.write().await = ViewerState::Connecting;
        info!("Starting USB viewer...");

        let capture = UsbCapture::with_config(usb_config);
        if !capture.test_connection().await? {
            return Err(Error::UsbConnection("Cannot connect to device".into()));
        }
        let info = capture.get_device_info().await?;
        info!("Connected to {} running firmware {}", info.model, info.firmware_version);

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
        match &self.source {
            ViewerSource::Usb(cfg) => UsbCapture::with_config(cfg.clone()).capture_frame().await,
            ViewerSource::Cloud(_) => self
                .subscribe()
                .recv()
                .await
                .map_err(|_| Error::Timeout("No frame received".into())),
        }
    }
}
