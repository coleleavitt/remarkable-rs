//! Screen share viewer: frames from the tablet over the cloud broker, or from
//! its framebuffer over USB.

use std::sync::atomic::{AtomicU64, Ordering};
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
    /// Bumped by every start, so a superseded background task can't
    /// overwrite the state of a newer one.
    generation: Arc<AtomicU64>,
    frame_tx: broadcast::Sender<Frame>,
}

/// Set `state` only while `gen` is still the current generation.
async fn set_state(state: &RwLock<ViewerState>, generation: &AtomicU64, gen: u64, value: ViewerState) {
    let mut guard = state.write().await;
    if generation.load(Ordering::SeqCst) == gen {
        *guard = value;
    }
}

impl ScreenShareViewer {
    pub fn new(source: ViewerSource) -> Self {
        let (frame_tx, _) = broadcast::channel(4);
        Self { source, state: Arc::new(RwLock::new(ViewerState::Disconnected)), generation: Arc::default(), frame_tx }
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
        let gen = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        *self.state.write().await = ViewerState::Connecting;
        let session = match crate::cloud::connect(cfg).await {
            Ok(s) => s,
            Err(e) => {
                error!("Cloud connect failed: {}", e);
                *self.state.write().await = ViewerState::Error;
                return Err(e);
            }
        };

        // Before spawning, so the task's Streaming state can't be overwritten.
        *self.state.write().await = ViewerState::Connected;
        let frame_tx = self.frame_tx.clone();
        let state = self.state.clone();
        let generation = self.generation.clone();
        tokio::spawn(async move {
            // Own the session here so the peer connection and signaling task live
            // exactly as long as frames are flowing.
            let crate::cloud::CloudSession { webrtc, mut data_rx, signaling_task } = session;
            set_state(&state, &generation, gen, ViewerState::Streaming).await;
            if let Err(e) = pump_frames(&mut data_rx, |frame| {
                let _ = frame_tx.send(frame);
            })
            .await
            {
                error!("Screen share stream ended: {}", e);
            }
            signaling_task.abort();
            let _ = webrtc.close().await;
            set_state(&state, &generation, gen, ViewerState::Disconnected).await;
        });
        Ok(())
    }

    async fn start_usb(&self, usb_config: UsbConfig) -> Result<()> {
        let gen = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        *self.state.write().await = ViewerState::Connecting;
        info!("Starting USB viewer...");

        let capture = UsbCapture::with_config(usb_config);
        let mut frame_rx = match Self::open_usb(&capture).await {
            Ok(rx) => rx,
            Err(e) => {
                *self.state.write().await = ViewerState::Error;
                return Err(e);
            }
        };
        *self.state.write().await = ViewerState::Connected;
        let frame_tx = self.frame_tx.clone();
        let state = self.state.clone();
        let generation = self.generation.clone();
        tokio::spawn(async move {
            set_state(&state, &generation, gen, ViewerState::Streaming).await;
            while let Some(frame) = frame_rx.recv().await {
                let _ = frame_tx.send(frame);
            }
            set_state(&state, &generation, gen, ViewerState::Disconnected).await;
        });
        Ok(())
    }

    async fn open_usb(capture: &UsbCapture) -> Result<tokio::sync::mpsc::Receiver<Frame>> {
        if !capture.test_connection().await? {
            return Err(Error::UsbConnection("Cannot connect to device".into()));
        }
        let info = capture.get_device_info().await?;
        info!("Connected to {} running firmware {}", info.model, info.firmware_version);
        capture.start_continuous(10).await
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
