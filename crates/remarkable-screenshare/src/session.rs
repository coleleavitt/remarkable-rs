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

/// A rectangle of a [`Frame`], in its pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Area {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// A picture of the tablet screen, rotated for display.
#[derive(Debug, Clone)]
pub struct Frame {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub format: PixelFormat,
    /// What changed since the previous frame (the whole frame after a
    /// handshake or rotation). Lets viewers repaint only that part.
    pub changed: Area,
    pub timestamp: Instant,
}

impl Frame {
    /// The whole frame as an [`Area`].
    pub fn full_area(&self) -> Area {
        Area { x: 0, y: 0, width: self.width, height: self.height }
    }
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
        // Union of this message's updates; `None` with `changed` means everything.
        let mut dirty: Option<crate::rfb::Rect> = None;
        let mut everything = false;
        let mut stopped = false;
        for event in decoder.feed(&data)? {
            match event {
                Event::Handshake { version, width, height } => {
                    info!("Screen share handshake: server v{version} {width}x{height}");
                    // A new framebuffer: the next frame replaces the whole picture.
                    everything = true;
                    on_update(Update::Connected { width, height });
                }
                Event::FramebufferUpdated { dirty: rect, rects } => {
                    debug!("Framebuffer update: {rects} rects, dirty {rect:?}");
                    // Updates whose rects were all skipped change nothing.
                    if rect.width > 0 && rect.height > 0 {
                        changed = true;
                        dirty = Some(dirty.map_or(rect, |d| d.union(rect)));
                    }
                }
                Event::Rotation(degrees) => {
                    info!("Tablet rotation: {degrees}°");
                    changed = decoder.is_ready();
                    everything = true;
                }
                Event::Cursor { x, y } => on_update(Update::Cursor(decoder.display_point(x, y))),
                Event::Ping => debug!("Ping"),
                Event::Shutdown => {
                    info!("Tablet stopped screen share");
                    // Deliver what came before it in this message first.
                    stopped = true;
                    break;
                }
            }
        }
        if changed {
            let (data, width, height, format) = decoder.frame();
            let changed = match dirty {
                Some(rect) if !everything => {
                    let (x, y, width, height) = decoder.display_rect(rect);
                    Area { x, y, width, height }
                }
                _ => Area { x: 0, y: 0, width, height },
            };
            on_update(Update::Frame(Frame { data, width, height, format, changed, timestamp: Instant::now() }));
        }
        if stopped {
            return Ok(());
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
    async fn frames_report_what_changed() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        tx.send(handshake(4, 2)).unwrap();
        tx.send(update(&[(Rect { x: 0, y: 0, width: 4, height: 2 }, 0)])).unwrap();
        tx.send(update(&[(Rect { x: 1, y: 1, width: 2, height: 1 }, 0xffff)])).unwrap();
        tx.send(vec![0x65]).unwrap();
        let mut areas = Vec::new();
        pump_frames(&mut rx, |u| if let Update::Frame(f) = u { areas.push(f.changed) }).await.unwrap();
        assert_eq!(areas, vec![
            Area { x: 0, y: 0, width: 4, height: 2 },
            Area { x: 1, y: 1, width: 2, height: 1 },
        ]);
    }

    #[tokio::test]
    async fn frame_in_the_shutdown_message_is_delivered() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut msg = handshake(2, 2);
        msg.extend(update(&[(Rect { x: 0, y: 0, width: 2, height: 2 }, 0)]));
        msg.push(0x65);
        tx.send(msg).unwrap();
        let mut frames = 0;
        pump_frames(&mut rx, |u| if let Update::Frame(_) = u { frames += 1 }).await.unwrap();
        assert_eq!(frames, 1);
    }

    #[tokio::test]
    async fn closed_channel_is_an_error() {
        let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
        drop(tx);
        assert!(pump_frames(&mut rx, |_| {}).await.is_err());
    }
}
