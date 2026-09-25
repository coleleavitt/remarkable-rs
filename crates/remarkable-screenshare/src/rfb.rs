//! reMarkable screen share wire protocol ("RFB v2") spoken over the
//! `screenshare` WebRTC data channel.
//!
//! Reverse engineered from the desktop app (reMarkable 3.28.1 win64,
//! `screenshare/core/src/rfb/clientprotocol.cpp`) and the tablet side
//! (xochitl `screenshare/core/src/rfb/serverprotocol.cpp`). All integers are
//! big-endian (Qt `QDataStream` defaults).
//!
//! Client -> server: exactly one message, the handshake
//! `b"reMarkable"` + `i16` version (2), 12 bytes. The server has no parser for anything
//! else: any further bytes from the client are a protocol error and the tablet
//! closes the channel. Sending the version as an `i32` makes the server read
//! version 0 (falling back to v1) and then reject the two leftover bytes.
//!
//! Server -> client, one type byte followed by:
//!
//! | type          | payload                                                   |
//! |---------------|-----------------------------------------------------------|
//! | `0x68` `'h'`  | handshake: `u16` server version, `u16` width, `u16` height |
//! | `0x00`        | framebuffer update: `u16` rect count, `u32` len, `len` bytes of zlib |
//! | `0x64` `'d'`  | cursor: `u16` x, `u16` y                                   |
//! | `0x65` `'e'`  | server shutting down                                       |
//! | `0x66` `'f'`  | rotation: v1 `u8` (1 = 0°, else 270°), v2+ `i32` degrees   |
//! | `0x67` `'g'`  | ping; the client gives up after 20 s without one           |
//!
//! The inflated framebuffer payload is `rect count` records of
//! `u16 x, u16 y, u16 w, u16 h, u32 n` followed by `n` bytes of little-endian
//! RGB565 pixels, `w * 2` bytes per row. Every update is a fresh zlib stream
//! that the tablet ends with `Z_PARTIAL_FLUSH`, never `Z_FINISH`.
//!
//! The format is unchanged from xochitl 3.10.2 through 3.29.0.148 (checked in
//! each version's `serverprotocol`: same type bytes, `i16` version with
//! fallback to 2, `QImage::Format_RGB16` pixels, per-update `deflateInit`).

use bytes::{Buf, BytesMut};
use flate2::{Decompress, FlushDecompress, Status};
use tracing::{debug, trace, warn};

use crate::error::{Error, Result};
use crate::session::PixelFormat;

/// Protocol version this client speaks.
pub const CLIENT_VERSION: i16 = 2;

/// Client handshake, sent once when the data channel opens.
pub const CLIENT_HANDSHAKE: &[u8] = b"reMarkable\x00\x02";

/// Label of the data channel the tablet opens for screen share.
pub const CHANNEL_LABEL: &str = "screenshare";

/// Largest framebuffer accepted from a handshake (a Paper Pro is 2160x2880).
pub const MAX_PIXELS: usize = 4096 * 4096;

/// The desktop client disconnects when no ping arrives for this long.
pub const PING_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

mod msg {
    pub const FRAMEBUFFER_UPDATE: u8 = 0x00;
    pub const CURSOR: u8 = 0x64;
    pub const SHUTDOWN: u8 = 0x65;
    pub const ROTATION: u8 = 0x66;
    pub const PING: u8 = 0x67;
    pub const HANDSHAKE: u8 = 0x68;
}

/// Rectangle in framebuffer coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl Rect {
    fn union(self, other: Rect) -> Rect {
        if self.width == 0 || self.height == 0 {
            return other;
        }
        let x0 = self.x.min(other.x);
        let y0 = self.y.min(other.y);
        let x1 = (self.x + self.width).max(other.x + other.width);
        let y1 = (self.y + self.height).max(other.y + other.height);
        Rect { x: x0, y: y0, width: x1 - x0, height: y1 - y0 }
    }
}

/// A decoded server message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Handshake { version: u16, width: u16, height: u16 },
    /// The framebuffer changed; `dirty` is the union of the updated rects.
    FramebufferUpdated { dirty: Rect, rects: u16 },
    Cursor { x: u16, y: u16 },
    /// Tablet rotation in degrees; display turns the other way (see [`RfbDecoder::rotation`]).
    Rotation(i32),
    Ping,
    Shutdown,
}

/// Incremental decoder for the server -> client byte stream.
///
/// Data-channel messages do not align with protocol messages, so bytes are
/// buffered until a whole message is available.
#[derive(Default)]
pub struct RfbDecoder {
    buffer: BytesMut,
    version: u16,
    width: u16,
    height: u16,
    rotation: i32,
    /// RGB565 pixels, row-major.
    framebuffer: Vec<u16>,
}

impl RfbDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed bytes from the data channel and decode every complete message.
    pub fn feed(&mut self, data: &[u8]) -> Result<Vec<Event>> {
        self.buffer.extend_from_slice(data);
        let mut events = Vec::new();
        while let Some(event) = self.parse_message()? {
            events.push(event);
        }
        Ok(events)
    }

    /// Decode one message, consuming its bytes only when it is complete.
    fn parse_message(&mut self) -> Result<Option<Event>> {
        let Some(&kind) = self.buffer.first() else {
            return Ok(None);
        };
        let body = &self.buffer[1..];
        let (event, len) = match kind {
            msg::HANDSHAKE => {
                if body.len() < 6 {
                    return Ok(None);
                }
                let version = be16(body, 0).max(1);
                let (width, height) = (be16(body, 2), be16(body, 4));
                let pixels = width as usize * height as usize;
                if pixels == 0 || pixels > MAX_PIXELS {
                    return Err(Error::RfbProtocol(format!("unsupported framebuffer size {width}x{height}")));
                }
                self.version = version;
                self.width = width;
                self.height = height;
                self.framebuffer = vec![0xffff; width as usize * height as usize];
                (Event::Handshake { version, width, height }, 6)
            }
            msg::FRAMEBUFFER_UPDATE => {
                if body.len() < 6 {
                    return Ok(None);
                }
                let rects = be16(body, 0);
                let compressed_len = be32(body, 2) as usize;
                if body.len() < 6 + compressed_len {
                    trace!("framebuffer update: have {} of {} bytes", body.len() - 6, compressed_len);
                    return Ok(None);
                }
                if self.framebuffer.is_empty() {
                    return Err(Error::RfbProtocol("framebuffer update before handshake".into()));
                }
                // xochitl sends the non-overlapping rects of a QRegion: at most
                // one 12-byte header per rect plus 2 bytes per framebuffer pixel.
                let limit = 12 * rects as usize + 2 * self.framebuffer.len();
                let raw = inflate(&body[6..6 + compressed_len], limit)?;
                let dirty = self.apply_rects(&raw, rects)?;
                (Event::FramebufferUpdated { dirty, rects }, 6 + compressed_len)
            }
            msg::CURSOR => {
                if body.len() < 4 {
                    return Ok(None);
                }
                (Event::Cursor { x: be16(body, 0), y: be16(body, 2) }, 4)
            }
            msg::SHUTDOWN => (Event::Shutdown, 0),
            msg::ROTATION if self.version <= 1 => {
                let Some(&flag) = body.first() else {
                    return Ok(None);
                };
                self.rotation = if flag == 1 { 0 } else { 270 };
                (Event::Rotation(self.rotation), 1)
            }
            msg::ROTATION => {
                if body.len() < 4 {
                    return Ok(None);
                }
                self.rotation = be32(body, 0) as i32;
                (Event::Rotation(self.rotation), 4)
            }
            msg::PING => (Event::Ping, 0),
            other => {
                return Err(Error::RfbProtocol(format!("unknown server message type 0x{other:02x}")));
            }
        };
        self.buffer.advance(1 + len);
        Ok(Some(event))
    }

    /// Copy the inflated rects into the framebuffer and return their union.
    fn apply_rects(&mut self, raw: &[u8], count: u16) -> Result<Rect> {
        let (fb_w, fb_h) = (self.width as usize, self.height as usize);
        let mut dirty = Rect::default();
        let mut off = 0;
        for i in 0..count {
            if raw.len() < off + 12 {
                return Err(Error::RfbProtocol(format!(
                    "inflated data missing header for rect {i}: {} < {}",
                    raw.len() - off,
                    12
                )));
            }
            let rect = Rect {
                x: be16(raw, off),
                y: be16(raw, off + 2),
                width: be16(raw, off + 4),
                height: be16(raw, off + 6),
            };
            let len = be32(raw, off + 8) as usize;
            off += 12;
            if raw.len() < off + len {
                return Err(Error::RfbProtocol(format!(
                    "inflated data missing pixels for rect {i}: {} < {len}",
                    raw.len() - off
                )));
            }
            let pixels = &raw[off..off + len];
            off += len;

            let (x, y, w, h) = (rect.x as usize, rect.y as usize, rect.width as usize, rect.height as usize);
            // Same checks as the desktop client: skip bad rects, keep going.
            if w == 0 || h == 0 {
                continue;
            }
            if x + w > fb_w || y + h > fb_h {
                warn!("update rect {rect:?} outside of {fb_w}x{fb_h} framebuffer");
                continue;
            }
            if w * h * 2 > pixels.len() {
                warn!("update rect {rect:?} larger than its {} pixel bytes", pixels.len());
                continue;
            }
            for (row, src) in pixels.chunks_exact(w * 2).take(h).enumerate() {
                let dst = &mut self.framebuffer[(y + row) * fb_w + x..][..w];
                for (d, s) in dst.iter_mut().zip(src.chunks_exact(2)) {
                    *d = u16::from_le_bytes([s[0], s[1]]);
                }
            }
            dirty = dirty.union(rect);
        }
        debug!("applied {count} rects, dirty {dirty:?}");
        Ok(dirty)
    }

    pub fn is_ready(&self) -> bool {
        !self.framebuffer.is_empty()
    }

    /// Framebuffer size as sent by the tablet (before rotation).
    pub fn dimensions(&self) -> (u16, u16) {
        (self.width, self.height)
    }

    /// Rotation as the tablet reports it, in degrees. The desktop app turns
    /// the image the other way (`rotation: -ScreenShareClient.rotation` in
    /// its screen share QML), so a report of 90 means a quarter turn
    /// anticlockwise for display.
    pub fn rotation(&self) -> i32 {
        self.rotation
    }

    /// Raw RGB565 framebuffer, row-major, `width * height` pixels.
    pub fn framebuffer(&self) -> &[u16] {
        &self.framebuffer
    }

    /// The framebuffer with the display rotation applied, as 8-bit gray when
    /// every pixel is gray (always on the rM1/rM2) and as RGB otherwise: the
    /// Paper Pro sends its colour screen in the same RGB565 format (xochitl
    /// 3.28 ferrari frame writer 0x7900D0 paints into QImage::Format_RGB16).
    pub fn frame(&self) -> (Vec<u8>, u32, u32, PixelFormat) {
        let (w, h) = (self.width as usize, self.height as usize);
        let fb = &self.framebuffer;
        let (pixels, dw, dh) = match self.clockwise_turn() {
            // `map` takes a destination pixel to its source pixel.
            90 => (rotate(fb, w, (h, w), |x, y| (y, h - 1 - x)), h, w),
            180 => (rotate(fb, w, (w, h), |x, y| (w - 1 - x, h - 1 - y)), w, h),
            270 => (rotate(fb, w, (h, w), |x, y| (w - 1 - y, x)), h, w),
            _ => (fb.clone(), w, h),
        };
        let (data, format) = if pixels.iter().all(|&p| is_gray565(p)) {
            (pixels.iter().map(|&p| rgb565_to_gray(p)).collect(), PixelFormat::Gray8)
        } else {
            (pixels.iter().flat_map(|&p| rgb565_to_rgb(p)).collect(), PixelFormat::Rgb8)
        };
        (data, dw as u32, dh as u32, format)
    }

    /// Clockwise degrees to turn the framebuffer for display: 0, 90, 180 or 270.
    fn clockwise_turn(&self) -> i32 {
        (360 - self.rotation.rem_euclid(360)) % 360
    }

    /// Where framebuffer point `(x, y)` lands in [`frame`](Self::frame)'s
    /// rotated image. `None` for (0, 0), which the tablet uses for "no cursor"
    /// (the desktop app doesn't draw it there), and for points off the screen.
    pub fn display_point(&self, x: u16, y: u16) -> Option<(u32, u32)> {
        let (w, h) = (u32::from(self.width), u32::from(self.height));
        let (x, y) = (u32::from(x), u32::from(y));
        if (x, y) == (0, 0) || x >= w || y >= h {
            return None;
        }
        Some(match self.clockwise_turn() {
            90 => (h - 1 - y, x),
            180 => (w - 1 - x, h - 1 - y),
            270 => (y, w - 1 - x),
            _ => (x, y),
        })
    }
}

fn be16(b: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([b[at], b[at + 1]])
}

fn be32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// Inflate one update's zlib data.
///
/// The tablet starts a new zlib stream per update and ends it with
/// `Z_PARTIAL_FLUSH`, so there is no end-of-stream marker; like the desktop
/// client, inflate until the input is used up.
///
/// Fails if the output exceeds `limit` bytes.
fn inflate(input: &[u8], limit: usize) -> Result<Vec<u8>> {
    // One byte of headroom: a buffer filled to exactly `limit + 1` proves the
    // output is too big, while exactly `limit` bytes still ends normally.
    let cap = limit + 1;
    let mut z = Decompress::new(true);
    let mut out = Vec::with_capacity((input.len() * 8).min(cap));
    loop {
        if out.len() == out.capacity() {
            if out.len() >= cap {
                return Err(Error::RfbProtocol(format!("inflated update larger than {limit} bytes")));
            }
            out.reserve(out.capacity().max(64 * 1024).min(cap - out.len()));
        }
        let consumed = z.total_in() as usize;
        let status = z
            .decompress_vec(&input[consumed..], &mut out, FlushDecompress::Sync)
            .map_err(|e| Error::RfbProtocol(format!("failed to read zstream: {e}")))?;
        // `reserve` may allocate more than asked, so check the output itself.
        if out.len() > limit {
            return Err(Error::RfbProtocol(format!("inflated update larger than {limit} bytes")));
        }
        let done_input = z.total_in() as usize == input.len();
        match status {
            Status::StreamEnd => return Ok(out),
            // All input consumed and output space left over: the flush point.
            _ if done_input && out.len() < out.capacity() => return Ok(out),
            Status::BufError if z.total_in() as usize == consumed && out.len() < out.capacity() => {
                return Err(Error::RfbProtocol("zstream made no progress".into()));
            }
            _ => {}
        }
    }
}

/// Gray in RGB565: what a gray value looks like after Qt converts it.
fn is_gray565(p: u16) -> bool {
    let (r, g, b) = ((p >> 11) & 0x1f, (p >> 5) & 0x3f, p & 0x1f);
    r == b && g >> 1 == r
}

fn rgb565_to_rgb(p: u16) -> [u8; 3] {
    let (r, g, b) = (((p >> 11) & 0x1f) as u8, ((p >> 5) & 0x3f) as u8, (p & 0x1f) as u8);
    [(r << 3) | (r >> 2), (g << 2) | (g >> 4), (b << 3) | (b >> 2)]
}

fn rgb565_to_gray(p: u16) -> u8 {
    let r = ((p >> 11) & 0x1f) as u32;
    let g = ((p >> 5) & 0x3f) as u32;
    let b = (p & 0x1f) as u32;
    let (r, g, b) = ((r << 3) | (r >> 2), (g << 2) | (g >> 4), (b << 3) | (b >> 2));
    ((r * 77 + g * 150 + b * 29) >> 8) as u8
}

/// Build a `dw`x`dh` image from `src` (row stride `w`); `map` takes a
/// destination pixel to its source pixel.
fn rotate<T: Copy>(src: &[T], w: usize, (dw, dh): (usize, usize), map: impl Fn(usize, usize) -> (usize, usize)) -> Vec<T> {
    let mut dst = Vec::with_capacity(src.len());
    for y in 0..dh {
        for x in 0..dw {
            let (sx, sy) = map(x, y);
            dst.push(src[sy * w + sx]);
        }
    }
    dst
}

#[cfg(test)]
pub(crate) mod testdata {
    use super::*;
    use flate2::{Compress, Compression, FlushCompress};

    pub(crate) fn handshake(w: u16, h: u16) -> Vec<u8> {
        let mut m = vec![msg::HANDSHAKE];
        m.extend(2u16.to_be_bytes());
        m.extend(w.to_be_bytes());
        m.extend(h.to_be_bytes());
        m
    }

    /// Compress like the tablet: one stream, `Z_PARTIAL_FLUSH` at the end, never finished.
    pub(crate) fn partial_flushed_zlib(raw: &[u8]) -> Vec<u8> {
        let mut z = Compress::new(Compression::default(), true);
        let mut out = Vec::with_capacity(raw.len() + 1024);
        z.compress_vec(raw, &mut out, FlushCompress::None).unwrap();
        loop {
            out.reserve(64 * 1024);
            let before = out.len();
            z.compress_vec(&[], &mut out, FlushCompress::Partial).unwrap();
            if out.len() == before || out.len() < out.capacity() {
                break;
            }
        }
        out
    }

    pub(crate) fn update(rects: &[(Rect, u16)]) -> Vec<u8> {
        let mut raw = Vec::new();
        for (r, px) in rects {
            for v in [r.x, r.y, r.width, r.height] {
                raw.extend(v.to_be_bytes());
            }
            let n = r.width as u32 * r.height as u32 * 2;
            raw.extend(n.to_be_bytes());
            for _ in 0..r.width as u32 * r.height as u32 {
                raw.extend(px.to_le_bytes());
            }
        }
        let z = partial_flushed_zlib(&raw);
        let mut m = vec![msg::FRAMEBUFFER_UPDATE];
        m.extend((rects.len() as u16).to_be_bytes());
        m.extend((z.len() as u32).to_be_bytes());
        m.extend(z);
        m
    }
}

#[cfg(test)]
mod tests {
    use super::testdata::{handshake, update};
    use super::*;

    #[test]
    fn handshake_then_update_split_across_chunks() {
        let r = Rect { x: 1, y: 1, width: 2, height: 1 };
        let mut stream = handshake(4, 3);
        stream.extend(update(&[(r, 0)]));
        stream.push(msg::PING);

        let mut d = RfbDecoder::new();
        let mut events = Vec::new();
        for byte in &stream {
            events.extend(d.feed(std::slice::from_ref(byte)).unwrap());
        }
        assert_eq!(
            events,
            vec![
                Event::Handshake { version: 2, width: 4, height: 3 },
                Event::FramebufferUpdated { dirty: r, rects: 1 },
                Event::Ping,
            ]
        );
        let (gray, w, h, _) = d.frame();
        assert_eq!((w, h), (4, 3));
        assert_eq!(gray[4 + 1], 0);
        assert_eq!(gray[4 + 2], 0);
        assert_eq!(gray[0], 255);
    }

    #[test]
    fn full_screen_update_inflates() {
        // Big enough that the inflated size dwarfs the compressed size.
        let (w, h) = (1404, 1872);
        let mut d = RfbDecoder::new();
        d.feed(&handshake(w, h)).unwrap();
        let full = Rect { x: 0, y: 0, width: w, height: h };
        let ev = d.feed(&update(&[(full, 0x0000)])).unwrap();
        assert_eq!(ev, vec![Event::FramebufferUpdated { dirty: full, rects: 1 }]);
        assert!(d.framebuffer().iter().all(|&p| p == 0));
    }

    #[test]
    fn out_of_bounds_rect_is_skipped() {
        let mut d = RfbDecoder::new();
        d.feed(&handshake(2, 2)).unwrap();
        let bad = Rect { x: 1, y: 0, width: 2, height: 1 };
        let ev = d.feed(&update(&[(bad, 0)])).unwrap();
        assert_eq!(ev, vec![Event::FramebufferUpdated { dirty: Rect::default(), rects: 1 }]);
        assert!(d.framebuffer().iter().all(|&p| p == 0xffff));
    }

    #[test]
    fn rotation_v2_and_gray_frame_rotates() {
        let mut d = RfbDecoder::new();
        d.feed(&handshake(3, 2)).unwrap();
        d.feed(&update(&[(Rect { x: 0, y: 0, width: 1, height: 1 }, 0)])).unwrap();
        let mut m = vec![msg::ROTATION];
        m.extend(90i32.to_be_bytes());
        assert_eq!(d.feed(&m).unwrap(), vec![Event::Rotation(90)]);
        let (gray, w, h, _) = d.frame();
        assert_eq!((w, h), (2, 3));
        // Reported 90 turns the image anticlockwise (as the desktop does):
        // the top-left source pixel lands bottom-left of the 2x3 result.
        assert_eq!(gray[2 * 2], 0);
        assert_eq!(gray.iter().filter(|&&p| p == 0).count(), 1);
    }

    #[test]
    fn client_handshake_is_header_plus_i16_version() {
        let mut expected = b"reMarkable".to_vec();
        expected.extend(CLIENT_VERSION.to_be_bytes());
        assert_eq!(CLIENT_HANDSHAKE, expected.as_slice());
        assert_eq!(CLIENT_HANDSHAKE.len(), 12);
    }

    #[test]
    fn display_point_follows_rotation() {
        let mut d = RfbDecoder::new();
        d.feed(&handshake(3, 2)).unwrap();
        assert_eq!(d.display_point(0, 0), None);
        assert_eq!(d.display_point(3, 0), None);
        assert_eq!(d.display_point(1, 1), Some((1, 1)));
        let mut m = vec![msg::ROTATION];
        m.extend(90i32.to_be_bytes());
        d.feed(&m).unwrap();
        // Same mapping as the pixels: a clockwise turn of a 3x2 image.
        let (gray, w, _) = {
            d.feed(&update(&[(Rect { x: 2, y: 1, width: 1, height: 1 }, 0)])).unwrap();
            let (g, w, h, _) = d.frame();
            (g, w, h)
        };
        let (px, py) = d.display_point(2, 1).unwrap();
        assert_eq!(gray[(py * w + px) as usize], 0);
    }

    #[test]
    fn colour_frames_come_out_as_rgb() {
        let mut d = RfbDecoder::new();
        d.feed(&handshake(2, 1)).unwrap();
        // Pure red (RGB565 0xF800) at (0, 0); the rest stays white.
        d.feed(&update(&[(Rect { x: 0, y: 0, width: 1, height: 1 }, 0xF800)])).unwrap();
        let (rgb, w, h, format) = d.frame();
        assert_eq!((w, h, format), (2, 1, PixelFormat::Rgb8));
        assert_eq!(rgb, vec![255, 0, 0, 255, 255, 255]);
    }

    #[test]
    fn gray_frames_stay_gray() {
        let mut d = RfbDecoder::new();
        d.feed(&handshake(2, 1)).unwrap();
        // Mid gray 0x80 as Qt stores it in RGB565: r5=16, g6=32, b5=16.
        let gray565 = (16 << 11) | (32 << 5) | 16;
        d.feed(&update(&[(Rect { x: 0, y: 0, width: 1, height: 1 }, gray565)])).unwrap();
        let (data, _, _, format) = d.frame();
        assert_eq!(format, PixelFormat::Gray8);
        assert_eq!(data.len(), 2);
    }

    #[test]
    fn oversized_handshake_is_rejected() {
        let mut d = RfbDecoder::new();
        assert!(d.feed(&handshake(u16::MAX, u16::MAX)).is_err());
        let mut d = RfbDecoder::new();
        assert!(d.feed(&handshake(0, 10)).is_err());
    }

    #[test]
    fn zero_sized_rect_is_skipped() {
        let mut d = RfbDecoder::new();
        d.feed(&handshake(2, 2)).unwrap();
        let empty = Rect { x: 0, y: 0, width: 0, height: 2 };
        let ev = d.feed(&update(&[(empty, 0)])).unwrap();
        assert_eq!(ev, vec![Event::FramebufferUpdated { dirty: Rect::default(), rects: 1 }]);
    }

    #[test]
    fn inflate_accepts_output_of_exactly_the_limit() {
        let raw = vec![7u8; 1000];
        let z = testdata::partial_flushed_zlib(&raw);
        assert_eq!(inflate(&z, 1000).unwrap(), raw);
        assert!(inflate(&z, 999).is_err());
    }

    #[test]
    fn zlib_bomb_is_rejected() {
        let mut d = RfbDecoder::new();
        d.feed(&handshake(4, 4)).unwrap();
        // 1 rect but megabytes of inflated zeros.
        let z = testdata::partial_flushed_zlib(&vec![0u8; 4 << 20]);
        let mut m = vec![msg::FRAMEBUFFER_UPDATE];
        m.extend(1u16.to_be_bytes());
        m.extend((z.len() as u32).to_be_bytes());
        m.extend(z);
        assert!(d.feed(&m).is_err());
    }

    #[test]
    fn unknown_type_is_an_error() {
        let mut d = RfbDecoder::new();
        assert!(d.feed(&[0x03]).is_err());
    }

    #[test]
    fn cursor_and_shutdown() {
        let mut d = RfbDecoder::new();
        let ev = d.feed(&[msg::CURSOR, 0, 5, 0, 7, msg::SHUTDOWN]).unwrap();
        assert_eq!(ev, vec![Event::Cursor { x: 5, y: 7 }, Event::Shutdown]);
    }
}
