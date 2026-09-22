//! USB Framebuffer Capture
//!
//! Direct screen capture from a USB-connected reMarkable device
//! by reading the framebuffer device via SSH.
//!
//! This bypasses the cloud entirely and works offline.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use thiserror::Error;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tracing::{debug, info, warn};

use crate::rfb::{FB_HEIGHT, FB_WIDTH};

/// USB host for reMarkable
pub const DEFAULT_HOST: &str = "10.11.99.1";
pub const DEFAULT_USER: &str = "root";

/// Framebuffer device path
pub const FB_DEVICE: &str = "/dev/fb0";

/// USB capture errors
#[derive(Error, Debug)]
pub enum UsbError {
    #[error("SSH connection failed: {0}")]
    Connection(String),

    #[error("Command failed: {0}")]
    Command(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid framebuffer data: expected {expected} bytes, got {actual}")]
    InvalidData { expected: usize, actual: usize },

    #[error("Device not found")]
    DeviceNotFound,
}

/// USB capture configuration
#[derive(Debug, Clone)]
pub struct UsbConfig {
    pub host: String,
    pub user: String,
    pub port: u16,
    pub identity_file: Option<PathBuf>,
}

impl Default for UsbConfig {
    fn default() -> Self {
        Self {
            host: DEFAULT_HOST.to_string(),
            user: DEFAULT_USER.to_string(),
            port: 22,
            identity_file: None,
        }
    }
}

impl UsbConfig {
    /// Create config with custom host
    pub fn with_host(mut self, host: impl Into<String>) -> Self {
        self.host = host.into();
        self
    }

    /// Create config with identity file
    pub fn with_identity(mut self, path: impl Into<PathBuf>) -> Self {
        self.identity_file = Some(path.into());
        self
    }
}

/// USB framebuffer capture client
pub struct UsbCapture {
    config: UsbConfig,
}

impl UsbCapture {
    /// Create new USB capture client
    pub fn new(config: UsbConfig) -> Self {
        Self { config }
    }

    /// Build SSH command with common options
    fn ssh_command(&self) -> Command {
        let mut cmd = Command::new("ssh");
        cmd.arg("-o").arg("StrictHostKeyChecking=no")
            .arg("-o").arg("UserKnownHostsFile=/dev/null")
            .arg("-o").arg("ConnectTimeout=5")
            .arg("-p").arg(self.config.port.to_string());

        if let Some(ref key) = self.config.identity_file {
            cmd.arg("-i").arg(key);
        }

        cmd.arg(format!("{}@{}", self.config.user, self.config.host));
        cmd
    }

    /// Check if device is reachable
    pub async fn check_connection(&self) -> Result<bool, UsbError> {
        let mut cmd = self.ssh_command();
        cmd.arg("echo ok");
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::null());

        let status = cmd.status().await?;
        Ok(status.success())
    }

    /// Get framebuffer info
    pub async fn get_fb_info(&self) -> Result<FramebufferInfo, UsbError> {
        let mut cmd = self.ssh_command();
        cmd.arg("fbset -i 2>/dev/null || cat /sys/class/graphics/fb0/virtual_size");
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::null());

        let output = cmd.output().await?;
        if !output.status.success() {
            // Return default dimensions
            return Ok(FramebufferInfo::default());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let info = FramebufferInfo::parse(&stdout);

        debug!(?info, "Got framebuffer info");

        Ok(info)
    }

    /// Capture single frame from framebuffer
    pub async fn capture_frame(&self) -> Result<Vec<u8>, UsbError> {
        let expected_size = (FB_WIDTH as usize) * (FB_HEIGHT as usize);

        let mut cmd = self.ssh_command();
        cmd.arg(format!("cat {}", FB_DEVICE));
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::null());

        let mut child = cmd.spawn()?;
        let mut stdout = child.stdout.take().unwrap();

        let mut data = Vec::with_capacity(expected_size);
        stdout.read_to_end(&mut data).await?;

        let status = child.wait().await?;
        if !status.success() {
            return Err(UsbError::Command("Failed to read framebuffer".into()));
        }

        // Framebuffer might be larger than display area
        if data.len() < expected_size {
            return Err(UsbError::InvalidData {
                expected: expected_size,
                actual: data.len(),
            });
        }

        // Extract just the display area
        Ok(data[..expected_size].to_vec())
    }

    /// Capture frames continuously
    pub async fn capture_continuous<F>(
        &self,
        mut callback: F,
        interval: Duration,
    ) -> Result<(), UsbError>
    where
        F: FnMut(&[u8]) -> bool,
    {
        info!(
            interval_ms = interval.as_millis(),
            "Starting continuous capture"
        );

        loop {
            match self.capture_frame().await {
                Ok(frame) => {
                    if !callback(&frame) {
                        info!("Capture stopped by callback");
                        break;
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Capture error");
                }
            }

            tokio::time::sleep(interval).await;
        }

        Ok(())
    }

    /// Run command on device
    pub async fn run_command(&self, command: &str) -> Result<String, UsbError> {
        let mut cmd = self.ssh_command();
        cmd.arg(command);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let output = cmd.output().await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(UsbError::Command(stderr.to_string()));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

/// Framebuffer info
#[derive(Debug, Clone)]
pub struct FramebufferInfo {
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub name: String,
}

impl Default for FramebufferInfo {
    fn default() -> Self {
        Self {
            width: FB_WIDTH as u32,
            height: FB_HEIGHT as u32,
            depth: 8,
            name: "remarkable".to_string(),
        }
    }
}

impl FramebufferInfo {
    /// Parse from fbset output
    fn parse(output: &str) -> Self {
        let mut info = Self::default();

        for line in output.lines() {
            if line.contains("geometry") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 5 {
                    if let Ok(w) = parts[1].parse() {
                        info.width = w;
                    }
                    if let Ok(h) = parts[2].parse() {
                        info.height = h;
                    }
                    if parts.len() >= 6 {
                        if let Ok(d) = parts[5].parse() {
                            info.depth = d;
                        }
                    }
                }
            } else if line.contains("Name") {
                if let Some(name) = line.split(':').nth(1) {
                    info.name = name.trim().to_string();
                }
            } else if line.contains(',') {
                // virtual_size format: "W,H"
                let parts: Vec<&str> = line.split(',').collect();
                if parts.len() >= 2 {
                    if let Ok(w) = parts[0].trim().parse() {
                        info.width = w;
                    }
                    if let Ok(h) = parts[1].trim().parse() {
                        info.height = h;
                    }
                }
            }
        }

        info
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = UsbConfig::default();
        assert_eq!(config.host, DEFAULT_HOST);
        assert_eq!(config.user, DEFAULT_USER);
        assert_eq!(config.port, 22);
    }

    #[test]
    fn test_parse_fbset() {
        let output = r#"
mode "1872x1404"
    geometry 1872 1404 1872 1404 8
    timings 0 0 0 0 0 0 0
endmode

Frame buffer device information:
    Name        : imx_epdc_fb
"#;
        let info = FramebufferInfo::parse(output);
        assert_eq!(info.width, 1872);
        assert_eq!(info.height, 1404);
        assert_eq!(info.depth, 8);
        assert_eq!(info.name, "imx_epdc_fb");
    }

    #[test]
    fn test_parse_virtual_size() {
        let output = "1872,1404";
        let info = FramebufferInfo::parse(output);
        assert_eq!(info.width, 1872);
        assert_eq!(info.height, 1404);
    }
}
