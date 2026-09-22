//! SSH-based device token extraction
//!
//! Extracts tokens from a reMarkable device's xochitl.conf via SSH.
//!
//! # Configuration Path
//! `/home/root/.config/remarkable/xochitl.conf`
//!
//! # Required Tokens
//! - `devicetoken` - Long-lived device authentication
//! - `usertoken` - Short-lived user session (expires ~3 hours)
//!
//! # Example
//!
//! ```ignore
//! use remarkable_mqtt::ssh::{SshConfig, extract_device_tokens};
//!
//! let config = SshConfig::default(); // root@10.11.99.1
//! let tokens = extract_device_tokens(&config).await?;
//! println!("Device token: {}...", &tokens.device_token[..50]);
//! ```

use std::process::Stdio;
use tokio::process::Command;

use crate::MqttError;

/// SSH configuration for device access
#[derive(Debug, Clone)]
pub struct SshConfig {
    /// Device hostname or IP (default: 10.11.99.1)
    pub host: String,
    /// SSH port (default: 22)
    pub port: u16,
    /// Username (default: root)
    pub user: String,
    /// SSH private key path (optional, uses default SSH agent if None)
    pub key_path: Option<String>,
}

impl Default for SshConfig {
    fn default() -> Self {
        Self {
            host: "10.11.99.1".to_string(),
            port: 22,
            user: "root".to_string(),
            key_path: None,
        }
    }
}

impl SshConfig {
    /// Create config with custom host
    pub fn with_host(mut self, host: impl Into<String>) -> Self {
        self.host = host.into();
        self
    }

    /// Create config with custom user
    pub fn with_user(mut self, user: impl Into<String>) -> Self {
        self.user = user.into();
        self
    }

    /// Create config with private key path
    pub fn with_key(mut self, key_path: impl Into<String>) -> Self {
        self.key_path = Some(key_path.into());
        self
    }

    /// Build SSH command args
    fn ssh_args(&self) -> Vec<String> {
        let mut args = vec![
            "-o".to_string(),
            "StrictHostKeyChecking=no".to_string(),
            "-o".to_string(),
            "UserKnownHostsFile=/dev/null".to_string(),
            "-o".to_string(),
            "ConnectTimeout=10".to_string(),
            "-p".to_string(),
            self.port.to_string(),
        ];

        if let Some(ref key) = self.key_path {
            args.push("-i".to_string());
            args.push(key.clone());
        }

        args.push(format!("{}@{}", self.user, self.host));
        args
    }
}

/// Tokens extracted from device
#[derive(Debug, Clone)]
pub struct DeviceTokens {
    /// Device token (JWT) - used as MQTT username
    pub device_token: String,
    /// User token (JWT) - used as MQTT password
    pub user_token: String,
}

/// Path to xochitl.conf on device
const XOCHITL_CONF_PATH: &str = "/home/root/.config/remarkable/xochitl.conf";

/// Extract device and user tokens from device via SSH
///
/// Connects to the device and reads tokens from xochitl.conf.
/// Requires SSH access to the device (via USB or network).
pub async fn extract_device_tokens(config: &SshConfig) -> Result<DeviceTokens, MqttError> {
    // Read xochitl.conf via SSH
    let conf_content = ssh_read_file(config, XOCHITL_CONF_PATH).await?;

    // Parse tokens from config
    let device_token = extract_config_value(&conf_content, "devicetoken")
        .ok_or_else(|| MqttError::TokenParse("devicetoken not found in xochitl.conf".into()))?;

    let user_token = extract_config_value(&conf_content, "usertoken")
        .ok_or_else(|| MqttError::TokenParse("usertoken not found in xochitl.conf".into()))?;

    Ok(DeviceTokens {
        device_token,
        user_token,
    })
}

/// Read a file from device via SSH
async fn ssh_read_file(config: &SshConfig, path: &str) -> Result<String, MqttError> {
    let mut args = config.ssh_args();
    args.push(format!("cat {}", path));

    let output = Command::new("ssh")
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| MqttError::Connection(format!("SSH command failed: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(MqttError::Connection(format!(
            "SSH read failed: {}",
            stderr
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Execute a command on device via SSH
pub async fn ssh_exec(config: &SshConfig, cmd: &str) -> Result<String, MqttError> {
    let mut args = config.ssh_args();
    args.push(cmd.to_string());

    let output = Command::new("ssh")
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| MqttError::Connection(format!("SSH command failed: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(MqttError::Connection(format!("SSH exec failed: {}", stderr)));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Check if device is reachable via SSH
pub async fn check_device_reachable(config: &SshConfig) -> bool {
    let mut args = config.ssh_args();
    args.push("echo ok".to_string());

    Command::new("ssh")
        .args(&args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Extract a value from xochitl.conf format
///
/// Format: `key=value` (one per line)
fn extract_config_value(conf: &str, key: &str) -> Option<String> {
    for line in conf.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix(key) {
            if let Some(value) = rest.strip_prefix('=') {
                return Some(value.trim().to_string());
            }
        }
    }
    None
}

/// Restart xochitl service on device
///
/// Required after updating xochitl.conf with new tokens.
pub async fn restart_xochitl(config: &SshConfig) -> Result<(), MqttError> {
    ssh_exec(config, "systemctl restart xochitl").await?;
    Ok(())
}

/// Get device serial number
pub async fn get_device_serial(config: &SshConfig) -> Result<String, MqttError> {
    let output = ssh_exec(config, "cat /sys/devices/soc0/serial_number").await?;
    Ok(output.trim().to_string())
}

/// Get device model
pub async fn get_device_model(config: &SshConfig) -> Result<String, MqttError> {
    let output = ssh_exec(config, "cat /sys/devices/soc0/machine").await?;
    Ok(output.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_config_value() {
        let conf = r#"
[General]
devicetoken=abc123
usertoken=xyz789
"#;
        assert_eq!(
            extract_config_value(conf, "devicetoken"),
            Some("abc123".to_string())
        );
        assert_eq!(
            extract_config_value(conf, "usertoken"),
            Some("xyz789".to_string())
        );
        assert_eq!(extract_config_value(conf, "missing"), None);
    }

    #[test]
    fn test_ssh_config_args() {
        let config = SshConfig::default();
        let args = config.ssh_args();
        assert!(args.contains(&"root@10.11.99.1".to_string()));

        let config_with_key = config.with_key("/path/to/key");
        let args = config_with_key.ssh_args();
        assert!(args.contains(&"-i".to_string()));
        assert!(args.contains(&"/path/to/key".to_string()));
    }
}
