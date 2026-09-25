//! USB Framebuffer Capture
//!
//! Direct framebuffer capture from USB-connected reMarkable device.
//! Bypasses cloud entirely for lowest latency.

use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, Mutex};
use tokio::time;
use tracing::warn;

use crate::error::{Error, Result};
pub use crate::session::Frame;

/// Tablet address on the USB network.
const USB_IP: &str = "10.11.99.1";
const USB_SSH_PORT: u16 = 22;
const USB_USER: &str = "root";
const FB_DEVICE_PATH: &str = "/dev/fb0";
/// Fallback framebuffer size when `fbset` can't be read (reMarkable 1/2, portrait).
const FB_WIDTH: u32 = 1404;
const FB_HEIGHT: u32 = 1872;

/// USB capture configuration
///
/// `#[non_exhaustive]` so new fields (like `depth`) can be added without
/// breaking downstream callers; construct it from [`UsbConfig::default`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct UsbConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: Option<String>,
    pub key_path: Option<String>,
    pub width: u32,
    pub height: u32,
    /// Bits per pixel: 8 = gray (reMarkable 1/2), 16 = RGB565 (Paper Pro).
    pub depth: u32,
    pub fb_device: String,
}

impl Default for UsbConfig {
    fn default() -> Self {
        Self {
            host: USB_IP.to_string(),
            port: USB_SSH_PORT,
            user: USB_USER.to_string(),
            password: None,
            key_path: None,
            width: FB_WIDTH,
            height: FB_HEIGHT,
            depth: 8,
            fb_device: FB_DEVICE_PATH.to_string(),
        }
    }
}

impl UsbConfig {
    /// A default config targeting `host` (the way to build one now that the
    /// struct is `#[non_exhaustive]`).
    pub fn for_host(host: impl Into<String>) -> Self {
        Self { host: host.into(), ..Self::default() }
    }

    /// This config updated from what the device actually reports.
    ///
    /// If detection failed, the caller's config is kept unchanged rather than
    /// overwritten with fallback values. Otherwise it takes the detected `depth`
    /// (the reason detection exists — a Paper Pro is 16-bit RGB565, not 8-bit
    /// gray) and auto-fills geometry only when it is still the default, so
    /// deliberately non-default dimensions (or a custom `fb_device`, which
    /// `fbset -i` does not describe) are respected rather than clobbered.
    pub(crate) fn with_device(self, info: &DeviceInfo) -> Self {
        if !info.detected {
            return self;
        }
        let default_geometry = self.width == FB_WIDTH && self.height == FB_HEIGHT;
        Self {
            depth: info.depth,
            width: if default_geometry { info.width } else { self.width },
            height: if default_geometry { info.height } else { self.height },
            ..self
        }
    }
}

/// Device information
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub model: String,
    pub firmware_version: String,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    /// Whether `width`/`height`/`depth` came from the device (via `fbset`)
    /// rather than the caller's fallback config.
    pub detected: bool,
}

/// USB framebuffer capture
pub struct UsbCapture {
    config: UsbConfig,
    running: Arc<Mutex<bool>>,
}

impl UsbCapture {
    /// Create new USB capture with default config
    pub fn new() -> Self {
        Self::with_config(UsbConfig::default())
    }
    
    /// Create with custom config
    pub fn with_config(config: UsbConfig) -> Self {
        Self {
            config,
            running: Arc::new(Mutex::new(false)),
        }
    }

    /// The configuration this capture uses.
    pub fn config(&self) -> &UsbConfig {
        &self.config
    }

    /// Query the device and return a capture configured for its real
    /// framebuffer geometry and pixel depth (so a Paper Pro is captured as
    /// 16-bit RGB565, not truncated to 8-bit gray).
    pub async fn detected(&self) -> Result<Self> {
        let info = self.get_device_info().await?;
        Ok(Self::with_config(self.config.clone().with_device(&info)))
    }

    /// Test SSH connection
    pub async fn test_connection(&self) -> Result<bool> {
        let output = self.run_ssh_command("echo ok").await?;
        Ok(output.trim() == "ok")
    }
    
    /// Get device information
    pub async fn get_device_info(&self) -> Result<DeviceInfo> {
        // Get device model from /etc/version
        let version_output = self.run_ssh_command("cat /etc/version 2>/dev/null || echo unknown").await?;
        let firmware_version = version_output.trim().to_string();
        
        // Get framebuffer info using fbset. If it gives us nothing, fall back to
        // the caller's configured geometry (not a hardcoded guess) and flag that
        // detection did not happen, so we don't mislabel the framebuffer.
        let fbset_output = self.run_ssh_command("fbset -i 2>/dev/null || true").await?;
        let (width, height, depth, detected) = match parse_fbset_output(&fbset_output) {
            Some((w, h, d)) => (w, h, d, true),
            None => (self.config.width, self.config.height, self.config.depth, false),
        };

        // Detect model from dimensions
        let model = match (width, height) {
            (1404, 1872) | (1872, 1404) => "reMarkable 2",
            (2880, 2160) | (2160, 2880) => "reMarkable Paper Pro",
            _ => "Unknown",
        }.to_string();

        Ok(DeviceInfo {
            model,
            firmware_version,
            width,
            height,
            depth,
            detected,
        })
    }
    
    /// Capture single frame
    pub async fn capture_frame(&self) -> Result<Frame> {
        let start = std::time::Instant::now();

        // Read framebuffer
        let raw_data = self.run_ssh_command_binary(&format!("cat {}", shell_quote(&self.config.fb_device))).await?;

        // Validate size: honour the pixel depth (Paper Pro is 2 bytes/pixel).
        // Only the two depths reMarkable devices use are supported; anything
        // else is rejected rather than silently truncated or mislabelled.
        let pixels = self.config.width as usize * self.config.height as usize;
        let bytes_per_pixel = match self.config.depth {
            8 => 1,
            16 => 2,
            other => return Err(Error::Framebuffer(format!("unsupported framebuffer depth {other} bpp"))),
        };
        let expected_size = pixels * bytes_per_pixel;
        if raw_data.len() < expected_size {
            return Err(Error::Framebuffer(format!(
                "Incomplete framebuffer: {} < {}",
                raw_data.len(),
                expected_size
            )));
        }

        let (data, format) = decode_framebuffer(&raw_data, pixels, self.config.depth);
        Ok(Frame {
            format,
            changed: crate::session::Area { x: 0, y: 0, width: self.config.width, height: self.config.height },
            data,
            width: self.config.width,
            height: self.config.height,
            timestamp: start,
        })
    }
    
    /// Start continuous capture
    pub async fn start_continuous(&self, fps: u32) -> Result<mpsc::Receiver<Frame>> {
        if fps == 0 {
            return Err(Error::Framebuffer("capture rate must be at least 1 fps".into()));
        }
        let (tx, rx) = mpsc::channel(2);
        let config = self.config.clone();
        let running = self.running.clone();
        
        *running.lock().await = true;
        
        tokio::spawn(async move {
            let interval = Duration::from_millis(1000 / fps as u64);
            
            while *running.lock().await {
                let capture = UsbCapture::with_config(config.clone());
                match capture.capture_frame().await {
                    Ok(frame) => {
                        if tx.send(frame).await.is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        warn!("Frame capture error: {}", e);
                    }
                }
                time::sleep(interval).await;
            }
        });
        
        Ok(rx)
    }
    
    /// Stop continuous capture
    pub async fn stop(&self) {
        *self.running.lock().await = false;
    }
    
    /// Run SSH command and get string output
    async fn run_ssh_command(&self, command: &str) -> Result<String> {
        let output = self.run_ssh_command_binary(command).await?;
        Ok(String::from_utf8_lossy(&output).to_string())
    }
    
    /// Run SSH command and get binary output
    async fn run_ssh_command_binary(&self, command: &str) -> Result<Vec<u8>> {
        let mut cmd = Command::new("ssh");
        
        // Add SSH options
        cmd.arg("-o").arg("StrictHostKeyChecking=no")
           .arg("-o").arg("UserKnownHostsFile=/dev/null")
           .arg("-o").arg("BatchMode=yes")
           .arg("-o").arg("ConnectTimeout=5");
        
        // Add key if specified
        if let Some(ref key) = self.config.key_path {
            cmd.arg("-i").arg(key);
        }
        
        // Add port if non-standard
        if self.config.port != 22 {
            cmd.arg("-p").arg(self.config.port.to_string());
        }
        
        // Add user@host
        cmd.arg(format!("{}@{}", self.config.user, self.config.host));
        
        // Add command
        cmd.arg(command);
        
        // Execute
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        
        let output = cmd.output()
            .map_err(|e| Error::Ssh(format!("Failed to execute ssh: {}", e)))?;
        
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::Ssh(format!("SSH command failed: {}", stderr)));
        }
        
        Ok(output.stdout)
    }
}

impl Default for UsbCapture {
    fn default() -> Self {
        Self::new()
    }
}

/// Decode `pixels` framebuffer pixels into a display frame's bytes and format.
///
/// `depth` is one of the two the caller has already validated: 16 is
/// little-endian RGB565 (the Paper Pro's colour screen), 8 is gray (reMarkable
/// 1/2). The caller has also checked `raw` holds at least
/// `pixels * bytes_per_pixel` bytes.
fn decode_framebuffer(raw: &[u8], pixels: usize, depth: u32) -> (Vec<u8>, crate::session::PixelFormat) {
    use crate::session::PixelFormat;
    if depth == 16 {
        let data = raw[..pixels * 2]
            .chunks_exact(2)
            .flat_map(|p| crate::rfb::rgb565_to_rgb(u16::from_le_bytes([p[0], p[1]])))
            .collect();
        (data, PixelFormat::Rgb8)
    } else {
        (raw[..pixels].to_vec(), PixelFormat::Gray8)
    }
}

/// Parse fbset output for `(width, height, depth)`; `None` if it has no usable
/// `geometry` line (so the caller can tell detection failed rather than silently
/// using a guess).
fn parse_fbset_output(output: &str) -> Option<(u32, u32, u32)> {
    for line in output.lines() {
        if line.contains("geometry") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 6 {
                let width = parts[1].parse().ok()?;
                let height = parts[2].parse().ok()?;
                let depth = parts[5].parse().ok()?;
                return Some((width, height, depth));
            }
        }
    }
    None
}

/// Quote `s` as one word for a POSIX shell.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn shell_quote_keeps_one_word() {
        assert_eq!(shell_quote("/dev/fb0"), "'/dev/fb0'");
        assert_eq!(shell_quote("x; rm -rf /"), "'x; rm -rf /'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn decodes_rgb565_when_16_bit() {
        // Pure red RGB565 (0xF800), little-endian bytes, one pixel.
        let (data, format) = decode_framebuffer(&[0x00, 0xF8], 1, 16);
        assert_eq!(format, crate::session::PixelFormat::Rgb8);
        assert_eq!(data, vec![255, 0, 0]);
    }

    #[test]
    fn decodes_gray_when_8_bit() {
        let (data, format) = decode_framebuffer(&[0x12, 0x34], 2, 8);
        assert_eq!(format, crate::session::PixelFormat::Gray8);
        assert_eq!(data, vec![0x12, 0x34]);
    }

    #[test]
    fn test_parse_fbset() {
        let output = r#"mode "1872x1404"
    geometry 1872 1404 1872 1404 8
"#;
        assert_eq!(parse_fbset_output(output), Some((1872, 1404, 8)));
    }

    #[test]
    fn parse_fbset_returns_none_without_geometry() {
        assert_eq!(parse_fbset_output(""), None);
        assert_eq!(parse_fbset_output("fbset: not found"), None);
    }

    #[test]
    fn with_device_keeps_config_when_detection_failed() {
        let cfg = UsbConfig { width: 100, height: 200, depth: 16, ..UsbConfig::default() };
        let info = DeviceInfo {
            model: "Unknown".into(),
            firmware_version: "x".into(),
            width: FB_WIDTH,
            height: FB_HEIGHT,
            depth: 8,
            detected: false,
        };
        let out = cfg.clone().with_device(&info);
        assert_eq!((out.width, out.height, out.depth), (100, 200, 16));
    }

    #[test]
    fn with_device_applies_detected_values() {
        let info = DeviceInfo {
            model: "reMarkable Paper Pro".into(),
            firmware_version: "x".into(),
            width: 2160,
            height: 2880,
            depth: 16,
            detected: true,
        };
        // A default-geometry config is corrected to the detected colour screen.
        let out = UsbConfig::default().with_device(&info);
        assert_eq!((out.width, out.height, out.depth), (2160, 2880, 16));
    }
}
