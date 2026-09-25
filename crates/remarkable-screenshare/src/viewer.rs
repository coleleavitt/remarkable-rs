//! Screen share viewer: frames from the tablet over the cloud broker, or from
//! its framebuffer over USB.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::sync::{broadcast, watch, RwLock};
use tracing::{error, info};

use crate::cloud::CloudConfig;
use crate::error::{Error, Result};
use crate::session::{pump_frames, Frame, Update};
use crate::usb::{UsbCapture, UsbConfig};
use crate::webrtc::WebRtcHandler;

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
    /// The running producer; a new start stops it.
    task: std::sync::Mutex<Option<tokio::task::AbortHandle>>,
    frame_tx: broadcast::Sender<Frame>,
    /// Pen position in the current frame's pixels (cloud sessions).
    cursor_tx: watch::Sender<Option<(u32, u32)>>,
}

/// Set `state` only while `gen` is still the current generation.
async fn set_state(state: &RwLock<ViewerState>, generation: &AtomicU64, gen: u64, value: ViewerState) {
    let mut guard = state.write().await;
    if generation.load(Ordering::SeqCst) == gen {
        *guard = value;
    }
}

/// Tears a cloud session down exactly once: aborts its MQTT signaling task and
/// closes its peer connection.
///
/// The normal path calls [`close`](Self::close) to await teardown. If the
/// producer task is aborted mid-stream instead (a newer `start` superseding it),
/// `Drop` still runs: it aborts the signaling task and spawns `webrtc.close()`
/// detached (a `Drop` can't await). Without this, an aborted producer never
/// reaches its cleanup and both the signaling task and the peer connection leak
/// on every replacement.
struct CloudCleanup {
    webrtc: Arc<WebRtcHandler>,
    signaling_task: Option<tokio::task::JoinHandle<()>>,
    done: bool,
}

impl CloudCleanup {
    fn new(webrtc: Arc<WebRtcHandler>, signaling_task: tokio::task::JoinHandle<()>) -> Self {
        Self { webrtc, signaling_task: Some(signaling_task), done: false }
    }

    /// Await teardown on the normal path.
    async fn close(mut self) {
        if let Some(task) = self.signaling_task.take() {
            task.abort();
        }
        let _ = self.webrtc.close().await;
        self.done = true;
    }
}

impl Drop for CloudCleanup {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        if let Some(task) = self.signaling_task.take() {
            task.abort();
        }
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let webrtc = self.webrtc.clone();
            handle.spawn(async move {
                let _ = webrtc.close().await;
            });
        }
    }
}

impl ScreenShareViewer {
    pub fn new(source: ViewerSource) -> Self {
        let (frame_tx, _) = broadcast::channel(4);
        Self {
            source,
            state: Arc::new(RwLock::new(ViewerState::Disconnected)),
            generation: Arc::default(),
            task: std::sync::Mutex::default(),
            frame_tx,
            cursor_tx: watch::channel(None).0,
        }
    }

    pub async fn state(&self) -> ViewerState {
        *self.state.read().await
    }

    /// Subscribe to frame updates
    pub fn subscribe(&self) -> broadcast::Receiver<Frame> {
        self.frame_tx.subscribe()
    }

    /// Follow the pen position; `None` means no cursor.
    pub fn cursor(&self) -> watch::Receiver<Option<(u32, u32)>> {
        self.cursor_tx.subscribe()
    }

    /// Install `producer` as the viewer's only background task, but only while
    /// `gen` is still the current generation; returns whether it was installed.
    ///
    /// The generation is checked while holding the task lock, so a start that was
    /// superseded after it began (a newer start already bumped the generation)
    /// can't install its producer and abort the newer one. When it returns
    /// `false` the `producer` future is dropped un-polled, running its own
    /// cleanup (e.g. [`CloudCleanup`]) on drop.
    fn replace_task(&self, gen: u64, producer: impl std::future::Future<Output = ()> + Send + 'static) -> bool {
        let mut slot = self.task.lock().unwrap_or_else(|e| e.into_inner());
        if self.generation.load(Ordering::SeqCst) != gen {
            return false;
        }
        let handle = tokio::spawn(producer).abort_handle();
        if let Some(old) = slot.replace(handle) {
            old.abort();
        }
        true
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

        // Build the cleanup guard now, before any task exists, so the session is
        // torn down even if the producer is aborted before its first poll or is
        // never installed: its `CloudCleanup` is dropped with the future.
        let crate::cloud::CloudSession { webrtc, mut data_rx, signaling_task } = session;
        let cleanup = CloudCleanup::new(webrtc, signaling_task);

        // A newer start superseded us while we were connecting: installing this
        // session would abort that newer producer, so tear this one down instead.
        if self.generation.load(Ordering::SeqCst) != gen {
            cleanup.close().await;
            return Ok(());
        }

        // Before spawning, so the task's Streaming state can't be overwritten.
        *self.state.write().await = ViewerState::Connected;
        let frame_tx = self.frame_tx.clone();
        let cursor_tx = self.cursor_tx.clone();
        let state = self.state.clone();
        let generation = self.generation.clone();
        // If a newer start slipped in while we took the state lock, `replace_task`
        // won't install this producer; it is dropped un-polled and its
        // `CloudCleanup` runs on drop, so nothing leaks.
        self.replace_task(gen, async move {
            let mut cleanup = cleanup;
            set_state(&state, &generation, gen, ViewerState::Streaming).await;
            // A fresh session starts with no cursor, clearing any left by a
            // superseded predecessor.
            if generation.load(Ordering::SeqCst) == gen {
                cursor_tx.send_replace(None);
            }
            if let Err(e) = pump_frames(&mut data_rx, |update| match update {
                Update::Frame(frame) => {
                    let _ = frame_tx.send(frame);
                }
                Update::Cursor(point) => {
                    // Don't publish a cursor once a newer session owns the view.
                    if generation.load(Ordering::SeqCst) == gen {
                        cursor_tx.send_replace(point);
                    }
                }
                Update::Connected { .. } => {}
            })
            .await
            {
                error!("Screen share stream ended: {}", e);
            }
            cleanup.close().await;
            // Drop any cursor when our stream ends, unless a newer one took over.
            if generation.load(Ordering::SeqCst) == gen {
                cursor_tx.send_replace(None);
            }
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
        self.replace_task(gen, async move {
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
        // Capture at the framebuffer's real geometry and depth: a Paper Pro is
        // 16-bit RGB565, not 8-bit gray.
        let capture = UsbCapture::with_config(capture.config().clone().with_device(&info));
        capture.start_continuous(10).await
    }

    /// Get a single frame
    pub async fn get_frame(&self) -> Result<Frame> {
        match &self.source {
            ViewerSource::Usb(cfg) => UsbCapture::with_config(cfg.clone()).detected().await?.capture_frame().await,
            ViewerSource::Cloud(_) => self
                .subscribe()
                .recv()
                .await
                .map_err(|_| Error::Timeout("No frame received".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usb::UsbConfig;
    use std::time::Duration;

    #[tokio::test]
    async fn a_new_task_stops_the_previous_one() {
        let viewer = ScreenShareViewer::new(ViewerSource::Usb(UsbConfig::default()));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let first = tx.clone();
        let g1 = viewer.generation.fetch_add(1, Ordering::SeqCst) + 1;
        assert!(viewer.replace_task(g1, async move {
            loop {
                let _ = first.send("first");
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }));
        tokio::time::sleep(Duration::from_millis(50)).await;
        let g2 = viewer.generation.fetch_add(1, Ordering::SeqCst) + 1;
        assert!(viewer.replace_task(g2, async {}));
        tokio::time::sleep(Duration::from_millis(50)).await;
        while rx.try_recv().is_ok() {}
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(rx.try_recv().is_err(), "the first task is still running");
    }

    #[tokio::test]
    async fn a_superseded_task_is_not_installed() {
        let viewer = ScreenShareViewer::new(ViewerSource::Usb(UsbConfig::default()));
        // A stale generation (a newer start already bumped it) must not install.
        let stale = viewer.generation.load(Ordering::SeqCst);
        viewer.generation.fetch_add(1, Ordering::SeqCst);
        assert!(!viewer.replace_task(stale, async {}));
    }
}
