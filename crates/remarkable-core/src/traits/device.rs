//! Device connection traits for USB, SSH, and WebUI access
//!
//! This module provides a unified interface for connecting to
//! reMarkable devices through various transport mechanisms.

use std::path::Path;
use std::time::Duration;

/// Device connection state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// Not connected
    Disconnected,
    /// Connection in progress
    Connecting,
    /// Connected and ready
    Connected,
    /// Reconnecting after failure
    Reconnecting,
    /// Connection failed
    Failed,
}

impl ConnectionState {
    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected)
    }
}

/// Device information
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    /// Device model (RM1, RM2, Paper Pro)
    pub model: String,
    /// Serial number
    pub serial: String,
    /// Firmware version
    pub firmware: String,
    /// Device storage info
    pub storage: Option<StorageInfo>,
    /// Battery level (0-100)
    pub battery: Option<u8>,
}

/// Storage information
#[derive(Debug, Clone)]
pub struct StorageInfo {
    /// Total bytes
    pub total: u64,
    /// Used bytes
    pub used: u64,
    /// Free bytes
    pub free: u64,
}

impl StorageInfo {
    pub fn usage_percent(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            self.used as f32 / self.total as f32 * 100.0
        }
    }
}

/// File entry from device filesystem
#[derive(Debug, Clone)]
pub struct FileEntry {
    /// File/directory name
    pub name: String,
    /// Full path on device
    pub path: String,
    /// Whether this is a directory
    pub is_dir: bool,
    /// File size in bytes (0 for directories)
    pub size: u64,
    /// Last modified timestamp
    pub modified: Option<String>,
}

/// Error type for device operations
#[derive(Debug, thiserror::Error)]
pub enum DeviceError {
    #[error("not connected")]
    NotConnected,
    
    #[error("connection failed: {0}")]
    ConnectionFailed(String),
    
    #[error("authentication failed: {0}")]
    AuthFailed(String),
    
    #[error("timeout after {0:?}")]
    Timeout(Duration),
    
    #[error("file not found: {0}")]
    FileNotFound(String),
    
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Core device connection trait
///
/// Implementations handle transport-specific connection details
/// while exposing a unified interface.
pub trait DeviceConnection: Send + Sync {
    /// Connect to the device
    fn connect(&mut self) -> impl std::future::Future<Output = Result<(), DeviceError>> + Send;
    
    /// Disconnect from the device
    fn disconnect(&mut self) -> impl std::future::Future<Output = Result<(), DeviceError>> + Send;
    
    /// Get current connection state
    fn state(&self) -> ConnectionState;
    
    /// Check if connected
    fn is_connected(&self) -> bool {
        self.state().is_connected()
    }
    
    /// Get device information
    fn device_info(&self) -> impl std::future::Future<Output = Result<DeviceInfo, DeviceError>> + Send;
}

/// File transfer operations
///
/// Extends DeviceConnection with filesystem operations.
pub trait FileTransfer: DeviceConnection {
    /// List files in a directory
    fn list_files(&self, path: &Path) -> impl std::future::Future<Output = Result<Vec<FileEntry>, DeviceError>> + Send;
    
    /// Download a file from the device
    fn download(&self, path: &Path) -> impl std::future::Future<Output = Result<Vec<u8>, DeviceError>> + Send;
    
    /// Upload a file to the device
    fn upload(&self, path: &Path, data: &[u8]) -> impl std::future::Future<Output = Result<(), DeviceError>> + Send;
    
    /// Delete a file or directory
    fn delete(&self, path: &Path) -> impl std::future::Future<Output = Result<(), DeviceError>> + Send;
    
    /// Create a directory
    fn mkdir(&self, path: &Path) -> impl std::future::Future<Output = Result<(), DeviceError>> + Send;
    
    /// Check if a path exists
    fn exists(&self, path: &Path) -> impl std::future::Future<Output = Result<bool, DeviceError>> + Send;
    
    /// Get file metadata
    fn stat(&self, path: &Path) -> impl std::future::Future<Output = Result<FileEntry, DeviceError>> + Send;
}

/// SSH connection configuration
#[derive(Debug, Clone)]
pub struct SshConfig {
    /// Hostname or IP address
    pub host: String,
    /// SSH port (default 22)
    pub port: u16,
    /// Username (default "root")
    pub username: String,
    /// Password (if using password auth)
    pub password: Option<String>,
    /// Path to private key
    pub key_path: Option<String>,
    /// Connection timeout
    pub timeout: Duration,
}

impl Default for SshConfig {
    fn default() -> Self {
        Self {
            host: "10.11.99.1".to_string(), // USB IP
            port: 22,
            username: "root".to_string(),
            password: None,
            key_path: None,
            timeout: Duration::from_secs(10),
        }
    }
}

impl SshConfig {
    /// Create config for USB connection
    pub fn usb() -> Self {
        Self::default()
    }
    
    /// Create config for WiFi connection
    pub fn wifi(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            ..Default::default()
        }
    }
    
    /// Set password
    pub fn with_password(mut self, password: impl Into<String>) -> Self {
        self.password = Some(password.into());
        self
    }
    
    /// Set key path
    pub fn with_key(mut self, path: impl Into<String>) -> Self {
        self.key_path = Some(path.into());
        self
    }
}

/// WebUI connection configuration
#[derive(Debug, Clone)]
pub struct WebUiConfig {
    /// Base URL (e.g., "http://10.11.99.1")
    pub url: String,
    /// Request timeout
    pub timeout: Duration,
}

impl Default for WebUiConfig {
    fn default() -> Self {
        Self {
            url: "http://10.11.99.1".to_string(),
            timeout: Duration::from_secs(30),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_ssh_config_builders() {
        let config = SshConfig::usb()
            .with_password("mypassword");
        assert_eq!(config.host, "10.11.99.1");
        assert_eq!(config.password, Some("mypassword".to_string()));
    }
    
    #[test]
    fn test_storage_usage() {
        let storage = StorageInfo {
            total: 1000,
            used: 250,
            free: 750,
        };
        assert!((storage.usage_percent() - 25.0).abs() < 0.01);
    }
}
