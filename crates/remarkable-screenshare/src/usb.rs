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
#[derive(Debug, Clone)]
pub struct UsbConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: Option<String>,
    pub key_path: Option<String>,
    pub width: u32,
    pub height: u32,
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
            fb_device: FB_DEVICE_PATH.to_string(),
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
        
        // Get framebuffer info using fbset
        let fbset_output = self.run_ssh_command("fbset -i 2>/dev/null || echo 'geometry 1872 1404 1872 1404 8'").await?;
        let (width, height, depth) = parse_fbset_output(&fbset_output);
        
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
        })
    }
    
    /// Capture single frame
    pub async fn capture_frame(&self) -> Result<Frame> {
        let start = std::time::Instant::now();
        
        // Read framebuffer
        let raw_data = self.run_ssh_command_binary(&format!("cat {}", shell_quote(&self.config.fb_device))).await?;
        
        // Validate size
        let expected_size = self.config.width as usize * self.config.height as usize;
        if raw_data.len() < expected_size {
            return Err(Error::Framebuffer(format!(
                "Incomplete framebuffer: {} < {}",
                raw_data.len(),
                expected_size
            )));
        }
        
        Ok(Frame {
            data: raw_data[..expected_size].to_vec(),
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

/// Parse fbset output to get dimensions
fn parse_fbset_output(output: &str) -> (u32, u32, u32) {
    let mut width = FB_WIDTH;
    let mut height = FB_HEIGHT;
    let mut depth = 8;
    
    for line in output.lines() {
        if line.contains("geometry") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 6 {
                width = parts[1].parse().unwrap_or(width);
                height = parts[2].parse().unwrap_or(height);
                depth = parts[5].parse().unwrap_or(depth);
            }
        }
    }
    
    (width, height, depth)
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
    fn test_parse_fbset() {
        let output = r#"mode "1872x1404"
    geometry 1872 1404 1872 1404 8
"#;
        let (w, h, d) = parse_fbset_output(output);
        assert_eq!(w, 1872);
        assert_eq!(h, 1404);
        assert_eq!(d, 8);
    }
}
