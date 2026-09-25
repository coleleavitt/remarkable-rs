//! Turn a screen share data channel into frames.

use std::time::Instant;

use tokio::sync::mpsc;
use tracing::{debug, info};

use crate::error::{Error, Result};
use crate::rfb::{Event, RfbDecoder, PING_TIMEOUT};

/// An 8-bit grayscale picture of the tablet screen, rotated for display.
#[derive(Debug, Clone)]
pub struct Frame {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub timestamp: Instant,
}

/// Decode the tablet's byte stream and call `on_frame` whenever the picture
/// changes (at most once per data-channel message).
///
/// Returns `Ok(())` when the tablet stops sharing, and an error when the
/// channel closes, pings stop for [`PING_TIMEOUT`], or the stream is malformed.
pub async fn pump_frames(
    data_rx: &mut mpsc::UnboundedReceiver<Vec<u8>>,
    mut on_frame: impl FnMut(Frame),
) -> Result<()> {
    let mut decoder = RfbDecoder::new();
    loop {
        // The tablet pings every second or so; silence means it is gone.
        let data = match tokio::time::timeout(PING_TIMEOUT, data_rx.recv()).await {
            Ok(Some(data)) => data,
            Ok(None) => return Err(Error::WebRtc("data channel closed".into())),
            Err(_) => return Err(Error::Timeout(format!("no data from tablet for {PING_TIMEOUT:?}"))),
        };
        let mut changed = false;
        for event in decoder.feed(&data)? {
            match event {
                Event::Handshake { version, width, height } => {
                    info!("Screen share handshake: server v{version} {width}x{height}");
                }
                Event::FramebufferUpdated { dirty, rects } => {
                    debug!("Framebuffer update: {rects} rects, dirty {dirty:?}");
                    // Updates whose rects were all skipped change nothing.
                    changed |= dirty.width > 0 && dirty.height > 0;
                }
                Event::Rotation(degrees) => {
                    info!("Tablet rotation: {degrees}°");
                    changed = decoder.is_ready();
                }
                Event::Cursor { x, y } => debug!("Cursor at {x},{y}"),
                Event::Ping => debug!("Ping"),
                Event::Shutdown => {
                    info!("Tablet stopped screen share");
                    return Ok(());
                }
            }
        }
        if changed {
            let (data, width, height) = decoder.gray_frame();
            on_frame(Frame { data, width, height, timestamp: Instant::now() });
        }
    }
}

#[cfg(feature = "app")]
impl Frame {
    /// The frame as an 8-bit grayscale image.
    pub fn to_gray_image(&self) -> image::GrayImage {
        image::ImageBuffer::from_raw(self.width, self.height, self.data.clone())
            .unwrap_or_else(|| image::GrayImage::new(self.width, self.height))
    }

    /// Save the frame as a PNG.
    pub fn save_png(&self, path: &std::path::Path) -> Result<()> {
        self.to_gray_image()
            .save(path)
            .map_err(|e| Error::Io(std::io::Error::other(e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rfb::testdata::{handshake, update};
    use crate::rfb::Rect;

    #[tokio::test]
    async fn frames_then_clean_shutdown() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        tx.send(handshake(4, 2)).unwrap();
        tx.send(update(&[(Rect { x: 0, y: 0, width: 4, height: 2 }, 0)])).unwrap();
        tx.send(vec![0x67]).unwrap(); // ping: no new frame
        tx.send(vec![0x65]).unwrap(); // tablet stops sharing

        let mut frames = Vec::new();
        pump_frames(&mut rx, |f| frames.push(f)).await.unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!((frames[0].width, frames[0].height), (4, 2));
        assert!(frames[0].data.iter().all(|&p| p == 0));
    }

    #[tokio::test]
    async fn closed_channel_is_an_error() {
        let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
        drop(tx);
        assert!(pump_frames(&mut rx, |_| {}).await.is_err());
    }
}
