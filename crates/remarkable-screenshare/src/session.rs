//! Turn a screen share data channel into frames.

use std::time::Instant;

use tokio::sync::mpsc;
use tracing::{debug, info};

use crate::error::{Error, Result};
use crate::rfb::{Event, RfbDecoder, PING_TIMEOUT};

/// How a [`Frame`]'s bytes are laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// One byte per pixel (reMarkable 1/2, and any all-gray screen).
    Gray8,
    /// Three bytes per pixel, R G B (colour screens such as the Paper Pro).
    Rgb8,
}

/// A picture of the tablet screen, rotated for display.
#[derive(Debug, Clone)]
pub struct Frame {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub format: PixelFormat,
    pub timestamp: Instant,
}

/// What [`pump_frames`] reports.
#[derive(Debug, Clone)]
pub enum Update {
    /// The tablet answered the handshake; the session is live.
    Connected { width: u16, height: u16 },
    /// The picture changed.
    Frame(Frame),
    /// The pen moved, in the rotated frame's pixels; `None` hides the cursor.
    Cursor(Option<(u32, u32)>),
}

/// Decode the tablet's byte stream and report each [`Update`]: a frame at most
/// once per data-channel message, cursor moves as they come.
///
/// Returns `Ok(())` when the tablet stops sharing, and an error when the
/// channel closes, pings stop for [`PING_TIMEOUT`], or the stream is malformed.
pub async fn pump_frames(
    data_rx: &mut mpsc::UnboundedReceiver<Vec<u8>>,
    mut on_update: impl FnMut(Update),
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
                    on_update(Update::Connected { width, height });
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
                Event::Cursor { x, y } => on_update(Update::Cursor(decoder.display_point(x, y))),
                Event::Ping => debug!("Ping"),
                Event::Shutdown => {
                    info!("Tablet stopped screen share");
                    return Ok(());
                }
            }
        }
        if changed {
            let (data, width, height, format) = decoder.frame();
            on_update(Update::Frame(Frame { data, width, height, format, timestamp: Instant::now() }));
        }
    }
}

#[cfg(feature = "app")]
impl Frame {
    /// The frame as an image in its own pixel format.
    pub fn to_image(&self) -> Result<image::DynamicImage> {
        let bad = || Error::Framebuffer(format!("{} bytes don't fit {}x{} {:?}", self.data.len(), self.width, self.height, self.format));
        Ok(match self.format {
            PixelFormat::Gray8 => image::DynamicImage::ImageLuma8(
                image::GrayImage::from_raw(self.width, self.height, self.data.clone()).ok_or_else(bad)?,
            ),
            PixelFormat::Rgb8 => image::DynamicImage::ImageRgb8(
                image::RgbImage::from_raw(self.width, self.height, self.data.clone()).ok_or_else(bad)?,
            ),
        })
    }

    /// Save the frame as a PNG.
    pub fn save_png(&self, path: &std::path::Path) -> Result<()> {
        self.to_image()?
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
        tx.send(vec![0x64, 0, 1, 0, 1]).unwrap(); // cursor: no new frame
        tx.send(vec![0x65]).unwrap(); // tablet stops sharing

        let mut frames = Vec::new();
        let mut connected = false;
        pump_frames(&mut rx, |u| match u {
            Update::Frame(f) => frames.push(f),
            Update::Connected { width, height } => connected = (width, height) == (4, 2),
            Update::Cursor(_) => {}
        })
        .await
        .unwrap();
        assert!(connected);
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
