//! Screen share client for reMarkable
//!
//! Supports:
//! - RFB/VNC (Screen Share V1, legacy)
//! - WebRTC (Screen Share V2, modern)

use std::io::{Read, Write};
use std::net::TcpStream;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ScreenError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Connection failed")]
    ConnectionFailed,
    #[error("Authentication failed")]
    AuthFailed,
    #[error("Protocol error: {0}")]
    Protocol(String),
}

/// RFB protocol version
pub const RFB_VERSION: &str = "RFB 003.008\n";

/// RFB security types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RfbSecurity {
    None = 1,
    VncAuth = 2,
}

/// RFB pixel format
#[derive(Debug, Clone)]
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
        Self {
            bits_per_pixel: 32,
            depth: 24,
            big_endian: false,
            true_color: true,
            red_max: 255,
            green_max: 255,
            blue_max: 255,
            red_shift: 16,
            green_shift: 8,
            blue_shift: 0,
        }
    }
}

/// Framebuffer update
#[derive(Debug, Clone)]
pub struct FramebufferUpdate {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
    pub encoding: i32,
    pub data: Vec<u8>,
}

/// RFB client for Screen Share V1
pub struct RfbClient {
    stream: Option<TcpStream>,
    width: u16,
    height: u16,
    name: String,
    pixel_format: PixelFormat,
}

impl RfbClient {
    pub fn new() -> Self {
        Self {
            stream: None,
            width: 0,
            height: 0,
            name: String::new(),
            pixel_format: PixelFormat::default(),
        }
    }
    
    /// Connect to RFB server
    pub fn connect(&mut self, host: &str, port: u16) -> Result<(), ScreenError> {
        let stream = TcpStream::connect((host, port))?;
        stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
        self.stream = Some(stream);
        
        self.handshake()?;
        Ok(())
    }
    
    fn handshake(&mut self) -> Result<(), ScreenError> {
        let stream = self.stream.as_mut().ok_or(ScreenError::ConnectionFailed)?;
        
        // Read server version
        let mut version = [0u8; 12];
        stream.read_exact(&mut version)?;
        
        // Send client version
        stream.write_all(RFB_VERSION.as_bytes())?;
        
        // Read security types
        let mut num_types = [0u8; 1];
        stream.read_exact(&mut num_types)?;
        
        if num_types[0] == 0 {
            return Err(ScreenError::ConnectionFailed);
        }
        
        let mut types = vec![0u8; num_types[0] as usize];
        stream.read_exact(&mut types)?;
        
        // Select security type (prefer None, fall back to VncAuth)
        let security = if types.contains(&1) {
            RfbSecurity::None
        } else if types.contains(&2) {
            RfbSecurity::VncAuth
        } else {
            return Err(ScreenError::AuthFailed);
        };
        
        stream.write_all(&[security as u8])?;
        
        // Handle VNC authentication if needed
        if security == RfbSecurity::VncAuth {
            // Read challenge
            let mut challenge = [0u8; 16];
            stream.read_exact(&mut challenge)?;
            
            // For now, send zeros (would need password encryption)
            stream.write_all(&[0u8; 16])?;
        }
        
        // Read security result
        let mut result = [0u8; 4];
        stream.read_exact(&mut result)?;
        
        if u32::from_be_bytes(result) != 0 {
            return Err(ScreenError::AuthFailed);
        }
        
        // Send client init (shared flag = 1)
        stream.write_all(&[1])?;
        
        // Read server init
        let mut init = [0u8; 24];
        stream.read_exact(&mut init)?;
        
        self.width = u16::from_be_bytes([init[0], init[1]]);
        self.height = u16::from_be_bytes([init[2], init[3]]);
        
        // Read name
        let name_len = u32::from_be_bytes([init[20], init[21], init[22], init[23]]) as usize;
        let mut name_bytes = vec![0u8; name_len];
        stream.read_exact(&mut name_bytes)?;
        self.name = String::from_utf8_lossy(&name_bytes).to_string();
        
        Ok(())
    }
    
    /// Request framebuffer update
    pub fn request_update(&mut self, incremental: bool) -> Result<(), ScreenError> {
        let stream = self.stream.as_mut().ok_or(ScreenError::ConnectionFailed)?;
        
        let mut msg = [0u8; 10];
        msg[0] = 3; // FramebufferUpdateRequest
        msg[1] = if incremental { 1 } else { 0 };
        // x, y = 0
        msg[6..8].copy_from_slice(&self.width.to_be_bytes());
        msg[8..10].copy_from_slice(&self.height.to_be_bytes());
        
        stream.write_all(&msg)?;
        Ok(())
    }
    
    /// Read framebuffer update
    pub fn read_update(&mut self) -> Result<Vec<FramebufferUpdate>, ScreenError> {
        let stream = self.stream.as_mut().ok_or(ScreenError::ConnectionFailed)?;
        
        let mut header = [0u8; 4];
        stream.read_exact(&mut header)?;
        
        if header[0] != 0 {
            return Err(ScreenError::Protocol(format!("Unexpected message type: {}", header[0])));
        }
        
        let num_rects = u16::from_be_bytes([header[2], header[3]]) as usize;
        let mut updates = Vec::with_capacity(num_rects);
        
        for _ in 0..num_rects {
            let mut rect = [0u8; 12];
            stream.read_exact(&mut rect)?;
            
            let x = u16::from_be_bytes([rect[0], rect[1]]);
            let y = u16::from_be_bytes([rect[2], rect[3]]);
            let w = u16::from_be_bytes([rect[4], rect[5]]);
            let h = u16::from_be_bytes([rect[6], rect[7]]);
            let encoding = i32::from_be_bytes([rect[8], rect[9], rect[10], rect[11]]);
            
            // Read pixel data (raw encoding = 0)
            let data_size = (w as usize) * (h as usize) * 4;
            let mut data = vec![0u8; data_size];
            stream.read_exact(&mut data)?;
            
            updates.push(FramebufferUpdate {
                x, y, width: w, height: h, encoding, data,
            });
        }
        
        Ok(updates)
    }
    
    /// Get framebuffer dimensions
    pub fn dimensions(&self) -> (u16, u16) {
        (self.width, self.height)
    }
    
    /// Get server name
    pub fn name(&self) -> &str {
        &self.name
    }
    
    /// Close connection
    pub fn close(&mut self) {
        self.stream = None;
    }
}

impl Default for RfbClient {
    fn default() -> Self {
        Self::new()
    }
}

/// WebRTC screen share (V2) - skeleton
pub struct WebRtcClient {
    // WebRTC state would go here
    // Requires libdatachannel or webrtc-rs
}

impl WebRtcClient {
    pub fn new() -> Self {
        Self {}
    }
    
    // WebRTC implementation would require:
    // 1. MQTT signaling connection
    // 2. ICE candidate exchange
    // 3. DataChannel setup
    // 4. RFB over DataChannel
}

impl Default for WebRtcClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_pixel_format_default() {
        let pf = PixelFormat::default();
        assert_eq!(pf.bits_per_pixel, 32);
        assert_eq!(pf.depth, 24);
        assert!(pf.true_color);
    }
    
    #[test]
    fn test_rfb_client_creation() {
        let client = RfbClient::new();
        assert_eq!(client.dimensions(), (0, 0));
    }
}
