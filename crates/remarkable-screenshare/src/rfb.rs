//! RFB (Remote Framebuffer) Protocol Implementation
//!
//! Based on RFB 3.8 specification used by reMarkable screen share.
//! The reMarkable uses RFB over WebRTC DataChannel.
//!
//! # Framebuffer Format
//! - Resolution: 1872x1404
//! - Bits per pixel: 8 (grayscale)
//! - Encoding: RAW or ZRLE

use std::io::{self, Cursor, Read};

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use flate2::read::ZlibDecoder;
use thiserror::Error;

/// Framebuffer dimensions for reMarkable 2
pub const FB_WIDTH: u16 = 1872;
pub const FB_HEIGHT: u16 = 1404;
pub const FB_BPP: u8 = 8;

/// RFB client-to-server message types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ClientMessage {
    SetPixelFormat = 0,
    SetEncodings = 2,
    FramebufferUpdateRequest = 3,
    KeyEvent = 4,
    PointerEvent = 5,
    ClientCutText = 6,
}

/// RFB server-to-client message types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ServerMessage {
    FramebufferUpdate = 0,
    SetColorMap = 1,
    Bell = 2,
    ServerCutText = 3,
}

impl TryFrom<u8> for ServerMessage {
    type Error = RfbError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::FramebufferUpdate),
            1 => Ok(Self::SetColorMap),
            2 => Ok(Self::Bell),
            3 => Ok(Self::ServerCutText),
            _ => Err(RfbError::InvalidMessageType(value)),
        }
    }
}

/// RFB encoding types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Encoding {
    Raw = 0,
    CopyRect = 1,
    Rre = 2,
    Hextile = 5,
    Trle = 15,
    Zrle = 16,
    Tight = 7,
    ZlibHex = 8,
    // Pseudo-encodings
    Cursor = -239,
    DesktopSize = -223,
}

impl TryFrom<i32> for Encoding {
    type Error = RfbError;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Raw),
            1 => Ok(Self::CopyRect),
            2 => Ok(Self::Rre),
            5 => Ok(Self::Hextile),
            15 => Ok(Self::Trle),
            16 => Ok(Self::Zrle),
            7 => Ok(Self::Tight),
            8 => Ok(Self::ZlibHex),
            -239 => Ok(Self::Cursor),
            -223 => Ok(Self::DesktopSize),
            _ => Err(RfbError::UnsupportedEncoding(value)),
        }
    }
}

/// RFB protocol errors
#[derive(Error, Debug)]
pub enum RfbError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Invalid message type: {0}")]
    InvalidMessageType(u8),

    #[error("Unsupported encoding: {0}")]
    UnsupportedEncoding(i32),

    #[error("Invalid rectangle: {x},{y} {width}x{height}")]
    InvalidRectangle {
        x: u16,
        y: u16,
        width: u16,
        height: u16,
    },

    #[error("Decompression error: {0}")]
    Decompression(String),

    #[error("Buffer too small: need {need}, have {have}")]
    BufferTooSmall { need: usize, have: usize },

    #[error("Protocol error: {0}")]
    Protocol(String),
}

/// RFB pixel format descriptor
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelFormat {
    pub bits_per_pixel: u8,
    pub depth: u8,
    pub big_endian: bool,
    pub true_color: bool,
    pub red_max: u16,
    pub green_max: u16,
    pub blue_max: u16,
    pub red_shift: u8,
    pub green_shift: u8,
    pub blue_shift: u8,
}

impl Default for PixelFormat {
    fn default() -> Self {
        Self::grayscale_8bit()
    }
}

impl PixelFormat {
    /// Standard 8-bit grayscale for reMarkable
    pub const fn grayscale_8bit() -> Self {
        Self {
            bits_per_pixel: 8,
            depth: 8,
            big_endian: false,
            true_color: true,
            red_max: 255,
            green_max: 255,
            blue_max: 255,
            red_shift: 0,
            green_shift: 0,
            blue_shift: 0,
        }
    }

    /// Encode pixel format to RFB binary (16 bytes)
    pub fn encode(&self) -> [u8; 16] {
        let mut buf = [0u8; 16];
        buf[0] = self.bits_per_pixel;
        buf[1] = self.depth;
        buf[2] = if self.big_endian { 1 } else { 0 };
        buf[3] = if self.true_color { 1 } else { 0 };
        buf[4..6].copy_from_slice(&self.red_max.to_be_bytes());
        buf[6..8].copy_from_slice(&self.green_max.to_be_bytes());
        buf[8..10].copy_from_slice(&self.blue_max.to_be_bytes());
        buf[10] = self.red_shift;
        buf[11] = self.green_shift;
        buf[12] = self.blue_shift;
        // bytes 13-15 are padding
        buf
    }

    /// Decode pixel format from RFB binary (16 bytes)
    pub fn decode(data: &[u8]) -> Result<Self, RfbError> {
        if data.len() < 16 {
            return Err(RfbError::BufferTooSmall {
                need: 16,
                have: data.len(),
            });
        }
        let mut cursor = Cursor::new(data);
        Ok(Self {
            bits_per_pixel: cursor.read_u8()?,
            depth: cursor.read_u8()?,
            big_endian: cursor.read_u8()? != 0,
            true_color: cursor.read_u8()? != 0,
            red_max: cursor.read_u16::<BigEndian>()?,
            green_max: cursor.read_u16::<BigEndian>()?,
            blue_max: cursor.read_u16::<BigEndian>()?,
            red_shift: cursor.read_u8()?,
            green_shift: cursor.read_u8()?,
            blue_shift: cursor.read_u8()?,
        })
    }

    /// Bytes per pixel
    pub fn bytes_per_pixel(&self) -> usize {
        (self.bits_per_pixel as usize + 7) / 8
    }
}

/// Framebuffer update rectangle
#[derive(Debug, Clone)]
pub struct Rectangle {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
    pub encoding: Encoding,
    pub data: Vec<u8>,
}

impl Rectangle {
    /// Total pixel count
    pub fn pixel_count(&self) -> usize {
        self.width as usize * self.height as usize
    }

    /// Decode raw-encoded rectangle data
    pub fn decode_raw(&self, pixel_format: &PixelFormat) -> Result<Vec<u8>, RfbError> {
        let expected = self.pixel_count() * pixel_format.bytes_per_pixel();
        if self.data.len() != expected {
            return Err(RfbError::BufferTooSmall {
                need: expected,
                have: self.data.len(),
            });
        }
        Ok(self.data.clone())
    }

    /// Decode ZRLE-encoded rectangle data
    pub fn decode_zrle(&self, pixel_format: &PixelFormat) -> Result<Vec<u8>, RfbError> {
        let mut decoder = ZlibDecoder::new(&self.data[..]);
        let mut decompressed = Vec::new();
        decoder
            .read_to_end(&mut decompressed)
            .map_err(|e| RfbError::Decompression(e.to_string()))?;

        // ZRLE uses 64x64 tiles
        let tile_size = 64;
        let mut output =
            vec![0u8; self.pixel_count() * pixel_format.bytes_per_pixel()];
        let mut cursor = Cursor::new(&decompressed);

        let tiles_x = (self.width as usize + tile_size - 1) / tile_size;
        let tiles_y = (self.height as usize + tile_size - 1) / tile_size;

        for ty in 0..tiles_y {
            for tx in 0..tiles_x {
                let tile_x = tx * tile_size;
                let tile_y = ty * tile_size;
                let tile_w = (self.width as usize - tile_x).min(tile_size);
                let tile_h = (self.height as usize - tile_y).min(tile_size);

                let subencoding = cursor.read_u8()?;

                if subencoding == 0 {
                    // Raw tile
                    for row in 0..tile_h {
                        let y_off = (tile_y + row) * self.width as usize;
                        for col in 0..tile_w {
                            let idx = y_off + tile_x + col;
                            output[idx] = cursor.read_u8()?;
                        }
                    }
                } else if subencoding == 1 {
                    // Solid tile
                    let color = cursor.read_u8()?;
                    for row in 0..tile_h {
                        let y_off = (tile_y + row) * self.width as usize;
                        for col in 0..tile_w {
                            let idx = y_off + tile_x + col;
                            output[idx] = color;
                        }
                    }
                } else {
                    // Palette or RLE subtypes - fall back to raw read
                    for row in 0..tile_h {
                        let y_off = (tile_y + row) * self.width as usize;
                        for col in 0..tile_w {
                            let idx = y_off + tile_x + col;
                            output[idx] = cursor.read_u8().unwrap_or(0);
                        }
                    }
                }
            }
        }

        Ok(output)
    }
}

/// Framebuffer update message
#[derive(Debug, Clone)]
pub struct FramebufferUpdate {
    pub rectangles: Vec<Rectangle>,
}

impl FramebufferUpdate {
    /// Parse framebuffer update from RFB data
    pub fn parse(data: &[u8]) -> Result<Self, RfbError> {
        let mut cursor = Cursor::new(data);

        // Message type (already consumed) + padding
        let msg_type = cursor.read_u8()?;
        if msg_type != ServerMessage::FramebufferUpdate as u8 {
            return Err(RfbError::InvalidMessageType(msg_type));
        }

        let _padding = cursor.read_u8()?;
        let num_rects = cursor.read_u16::<BigEndian>()?;

        let mut rectangles = Vec::with_capacity(num_rects as usize);

        for _ in 0..num_rects {
            let x = cursor.read_u16::<BigEndian>()?;
            let y = cursor.read_u16::<BigEndian>()?;
            let width = cursor.read_u16::<BigEndian>()?;
            let height = cursor.read_u16::<BigEndian>()?;
            let encoding_raw = cursor.read_i32::<BigEndian>()?;
            let encoding = Encoding::try_from(encoding_raw)?;

            // Calculate data size based on encoding
            let data_size = match encoding {
                Encoding::Raw => width as usize * height as usize,
                Encoding::Zrle => {
                    // ZRLE has a length prefix
                    let len = cursor.read_u32::<BigEndian>()? as usize;
                    len
                }
                _ => {
                    // For other encodings, try to read remaining
                    data.len() - cursor.position() as usize
                }
            };

            let mut rect_data = vec![0u8; data_size];
            cursor.read_exact(&mut rect_data)?;

            rectangles.push(Rectangle {
                x,
                y,
                width,
                height,
                encoding,
                data: rect_data,
            });
        }

        Ok(Self { rectangles })
    }
}

/// RFB message encoder for client-to-server messages
pub struct RfbEncoder;

impl RfbEncoder {
    /// Encode SetPixelFormat message
    pub fn set_pixel_format(format: &PixelFormat) -> Vec<u8> {
        let mut buf = Vec::with_capacity(20);
        buf.push(ClientMessage::SetPixelFormat as u8);
        buf.extend_from_slice(&[0, 0, 0]); // padding
        buf.extend_from_slice(&format.encode());
        buf
    }

    /// Encode SetEncodings message
    pub fn set_encodings(encodings: &[Encoding]) -> Vec<u8> {
        let mut buf = Vec::with_capacity(4 + encodings.len() * 4);
        buf.push(ClientMessage::SetEncodings as u8);
        buf.push(0); // padding
        buf.write_u16::<BigEndian>(encodings.len() as u16).unwrap();
        for enc in encodings {
            buf.write_i32::<BigEndian>(*enc as i32).unwrap();
        }
        buf
    }

    /// Encode FramebufferUpdateRequest message
    pub fn fb_update_request(
        incremental: bool,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
    ) -> Vec<u8> {
        let mut buf = Vec::with_capacity(10);
        buf.push(ClientMessage::FramebufferUpdateRequest as u8);
        buf.push(if incremental { 1 } else { 0 });
        buf.write_u16::<BigEndian>(x).unwrap();
        buf.write_u16::<BigEndian>(y).unwrap();
        buf.write_u16::<BigEndian>(width).unwrap();
        buf.write_u16::<BigEndian>(height).unwrap();
        buf
    }

    /// Encode KeyEvent message
    pub fn key_event(down: bool, key: u32) -> Vec<u8> {
        let mut buf = Vec::with_capacity(8);
        buf.push(ClientMessage::KeyEvent as u8);
        buf.push(if down { 1 } else { 0 });
        buf.extend_from_slice(&[0, 0]); // padding
        buf.write_u32::<BigEndian>(key).unwrap();
        buf
    }

    /// Encode PointerEvent message
    pub fn pointer_event(button_mask: u8, x: u16, y: u16) -> Vec<u8> {
        let mut buf = Vec::with_capacity(6);
        buf.push(ClientMessage::PointerEvent as u8);
        buf.push(button_mask);
        buf.write_u16::<BigEndian>(x).unwrap();
        buf.write_u16::<BigEndian>(y).unwrap();
        buf
    }
}

/// RFB protocol decoder state machine
#[derive(Clone)]
pub struct RfbDecoder {
    pixel_format: PixelFormat,
    framebuffer: Vec<u8>,
    width: u16,
    height: u16,
}

impl RfbDecoder {
    /// Create new decoder for reMarkable dimensions
    pub fn new() -> Self {
        Self::with_dimensions(FB_WIDTH, FB_HEIGHT)
    }

    /// Create decoder with custom dimensions
    pub fn with_dimensions(width: u16, height: u16) -> Self {
        let size = width as usize * height as usize;
        Self {
            pixel_format: PixelFormat::grayscale_8bit(),
            framebuffer: vec![0u8; size],
            width,
            height,
        }
    }

    /// Get current framebuffer
    pub fn framebuffer(&self) -> &[u8] {
        &self.framebuffer
    }

    /// Get framebuffer dimensions
    pub fn dimensions(&self) -> (u16, u16) {
        (self.width, self.height)
    }

    /// Get pixel format
    pub fn pixel_format(&self) -> &PixelFormat {
        &self.pixel_format
    }

    /// Process incoming server message
    pub fn process_message(&mut self, data: &[u8]) -> Result<bool, RfbError> {
        if data.is_empty() {
            return Ok(false);
        }

        let msg_type = ServerMessage::try_from(data[0])?;

        match msg_type {
            ServerMessage::FramebufferUpdate => {
                let update = FramebufferUpdate::parse(data)?;
                self.apply_update(&update)?;
                Ok(true)
            }
            ServerMessage::Bell => {
                tracing::debug!("Bell received");
                Ok(false)
            }
            ServerMessage::SetColorMap => {
                tracing::debug!("SetColorMap received (ignored for grayscale)");
                Ok(false)
            }
            ServerMessage::ServerCutText => {
                tracing::debug!("ServerCutText received (ignored)");
                Ok(false)
            }
        }
    }

    /// Apply framebuffer update to internal buffer
    fn apply_update(&mut self, update: &FramebufferUpdate) -> Result<(), RfbError> {
        for rect in &update.rectangles {
            // Validate rectangle bounds
            if rect.x as u32 + rect.width as u32 > self.width as u32
                || rect.y as u32 + rect.height as u32 > self.height as u32
            {
                return Err(RfbError::InvalidRectangle {
                    x: rect.x,
                    y: rect.y,
                    width: rect.width,
                    height: rect.height,
                });
            }

            // Decode rectangle data
            let pixels = match rect.encoding {
                Encoding::Raw => rect.decode_raw(&self.pixel_format)?,
                Encoding::Zrle => rect.decode_zrle(&self.pixel_format)?,
                _ => {
                    tracing::warn!(encoding = ?rect.encoding, "Unsupported encoding, skipping");
                    continue;
                }
            };

            // Copy pixels to framebuffer
            for row in 0..rect.height as usize {
                let src_offset = row * rect.width as usize;
                let dst_offset =
                    (rect.y as usize + row) * self.width as usize + rect.x as usize;

                let src_end = src_offset + rect.width as usize;
                let dst_end = dst_offset + rect.width as usize;

                if src_end <= pixels.len() && dst_end <= self.framebuffer.len() {
                    self.framebuffer[dst_offset..dst_end]
                        .copy_from_slice(&pixels[src_offset..src_end]);
                }
            }
        }

        Ok(())
    }

    /// Reset framebuffer to black
    pub fn clear(&mut self) {
        self.framebuffer.fill(0);
    }
}

impl Default for RfbDecoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pixel_format_roundtrip() {
        let format = PixelFormat::grayscale_8bit();
        let encoded = format.encode();
        let decoded = PixelFormat::decode(&encoded).unwrap();
        assert_eq!(format, decoded);
    }

    #[test]
    fn test_fb_update_request_encoding() {
        let msg = RfbEncoder::fb_update_request(true, 0, 0, FB_WIDTH, FB_HEIGHT);
        assert_eq!(msg[0], ClientMessage::FramebufferUpdateRequest as u8);
        assert_eq!(msg[1], 1); // incremental
        assert_eq!(msg.len(), 10);
    }

    #[test]
    fn test_decoder_dimensions() {
        let decoder = RfbDecoder::new();
        assert_eq!(decoder.dimensions(), (FB_WIDTH, FB_HEIGHT));
        assert_eq!(
            decoder.framebuffer().len(),
            FB_WIDTH as usize * FB_HEIGHT as usize
        );
    }
}
