//! D-Bus client for reMarkable device control
//!
//! Interfaces with device services via D-Bus over SSH.
//! Services: sync, settings, power, update, etc.

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DbusError {
    #[error("Connection error: {0}")]
    Connection(String),
    #[error("Method call failed: {0}")]
    MethodCall(String),
    #[error("Property access failed: {0}")]
    Property(String),
    #[error("Service not available: {0}")]
    ServiceNotAvailable(String),
}

/// D-Bus service names on reMarkable
pub mod services {
    pub const SYNC: &str = "no.remarkable.sync";
    pub const SETTINGS: &str = "no.remarkable.settings";
    pub const POWER: &str = "no.remarkable.power";
    pub const UPDATE: &str = "no.remarkable.update";
    pub const XOCHITL: &str = "no.remarkable.xochitl";
    pub const MDM: &str = "com.remarkable.devicepolicy.MDMAgent1";
}

/// D-Bus object paths
pub mod paths {
    pub const SYNC: &str = "/no/remarkable/sync/Synchronizer";
    pub const SETTINGS: &str = "/no/remarkable/settings/Settings";
    pub const POWER: &str = "/no/remarkable/power/Power";
    pub const MDM: &str = "/com/remarkable/devicepolicy/MDMAgent1";
}

/// Sync status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncStatus {
    Idle,
    Syncing,
    Error,
    Offline,
}

/// Power state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PowerState {
    Active,
    Suspended,
    Sleeping,
    PoweringOff,
    Charging,
}

/// D-Bus command to execute via SSH
#[derive(Debug, Clone)]
pub struct DbusCommand {
    pub service: String,
    pub path: String,
    pub interface: String,
    pub method: String,
    pub args: Vec<String>,
}

impl DbusCommand {
    pub fn new(service: &str, path: &str, interface: &str, method: &str) -> Self {
        Self {
            service: service.to_string(),
            path: path.to_string(),
            interface: interface.to_string(),
            method: method.to_string(),
            args: Vec::new(),
        }
    }
    
    pub fn arg(mut self, arg: &str) -> Self {
        self.args.push(arg.to_string());
        self
    }
    
    /// Generate dbus-send command string
    pub fn to_dbus_send(&self) -> String {
        let mut cmd = format!(
            "dbus-send --system --print-reply --dest={} {} {}.{}",
            self.service, self.path, self.interface, self.method
        );
        
        for arg in &self.args {
            cmd.push(' ');
            cmd.push_str(arg);
        }
        
        cmd
    }
    
    /// Generate gdbus command string (alternative)
    pub fn to_gdbus(&self) -> String {
        let args_str = if self.args.is_empty() {
            String::new()
        } else {
            format!(" {}", self.args.join(" "))
        };
        
        format!(
            "gdbus call --system --dest {} --object-path {} --method {}.{}{}",
            self.service, self.path, self.interface, self.method, args_str
        )
    }
}

/// Sync service commands
pub mod sync {
    use super::*;
    
    /// Trigger sync
    pub fn execute() -> DbusCommand {
        DbusCommand::new(
            services::SYNC,
            paths::SYNC,
            "no.remarkable.sync.Synchronizer",
            "execute",
        )
    }
    
    /// Abort sync
    pub fn abort() -> DbusCommand {
        DbusCommand::new(
            services::SYNC,
            paths::SYNC,
            "no.remarkable.sync.Synchronizer",
            "requestAbortAndCleanup",
        )
    }
    
    /// Release entry lock
    pub fn release_lock(entry_id: &str) -> DbusCommand {
        DbusCommand::new(
            services::SYNC,
            paths::SYNC,
            "no.remarkable.sync.Synchronizer",
            "releaseEntryLock",
        ).arg(&format!("string:{}", entry_id))
    }
    
    /// Set protocol flag
    pub fn set_protocol_flag(flag: &str, value: bool) -> DbusCommand {
        DbusCommand::new(
            services::SYNC,
            paths::SYNC,
            "no.remarkable.sync.Synchronizer",
            "setProtocolFlag",
        )
        .arg(&format!("string:{}", flag))
        .arg(&format!("boolean:{}", value))
    }
}

/// Power service commands
pub mod power {
    use super::*;
    
    /// Get battery level
    pub fn battery_level() -> DbusCommand {
        DbusCommand::new(
            services::POWER,
            paths::POWER,
            "org.freedesktop.DBus.Properties",
            "Get",
        )
        .arg("string:no.remarkable.power.Power")
        .arg("string:BatteryLevel")
    }
    
    /// Get charging status
    pub fn charging() -> DbusCommand {
        DbusCommand::new(
            services::POWER,
            paths::POWER,
            "org.freedesktop.DBus.Properties",
            "Get",
        )
        .arg("string:no.remarkable.power.Power")
        .arg("string:Charging")
    }
    
    /// Suspend device
    pub fn suspend() -> DbusCommand {
        DbusCommand::new(
            services::POWER,
            paths::POWER,
            "no.remarkable.power.Power",
            "Suspend",
        )
    }
    
    /// Reboot device
    pub fn reboot() -> DbusCommand {
        DbusCommand::new(
            services::POWER,
            paths::POWER,
            "no.remarkable.power.Power",
            "Reboot",
        )
    }
}

/// MDM (Mobile Device Management) commands
pub mod mdm {
    use super::*;
    
    /// Get MDM status
    pub fn status() -> DbusCommand {
        DbusCommand::new(
            services::MDM,
            paths::MDM,
            "org.freedesktop.DBus.Properties",
            "GetAll",
        )
        .arg("string:com.remarkable.devicepolicy.MDMAgent1")
    }
    
    /// Set SSH policy
    pub fn set_ssh_policy(enabled: bool) -> DbusCommand {
        DbusCommand::new(
            services::MDM,
            paths::MDM,
            "com.remarkable.devicepolicy.MDMAgent1",
            "SetSSHPolicy",
        )
        .arg(&format!("boolean:{}", enabled))
    }
    
    /// Set pincode policy
    pub fn set_pincode_policy(min_length: u32) -> DbusCommand {
        DbusCommand::new(
            services::MDM,
            paths::MDM,
            "com.remarkable.devicepolicy.MDMAgent1",
            "SetPincodePolicy",
        )
        .arg(&format!("uint32:{}", min_length))
    }
}

/// Update service commands
pub mod update {
    use super::*;
    
    /// Check for updates
    pub fn check() -> DbusCommand {
        DbusCommand::new(
            services::UPDATE,
            "/no/remarkable/update/Update",
            "no.remarkable.update.Update",
            "CheckForUpdates",
        )
    }
    
    /// Get current version
    pub fn version() -> DbusCommand {
        DbusCommand::new(
            services::UPDATE,
            "/no/remarkable/update/Update",
            "org.freedesktop.DBus.Properties",
            "Get",
        )
        .arg("string:no.remarkable.update.Update")
        .arg("string:CurrentVersion")
    }
}

/// Helper to list all D-Bus services
pub fn list_services() -> DbusCommand {
    DbusCommand {
        service: "org.freedesktop.DBus".to_string(),
        path: "/org/freedesktop/DBus".to_string(),
        interface: "org.freedesktop.DBus".to_string(),
        method: "ListNames".to_string(),
        args: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_sync_execute() {
        let cmd = sync::execute();
        let dbus_send = cmd.to_dbus_send();
        assert!(dbus_send.contains("no.remarkable.sync"));
        assert!(dbus_send.contains("execute"));
    }
    
    #[test]
    fn test_power_battery() {
        let cmd = power::battery_level();
        let dbus_send = cmd.to_dbus_send();
        assert!(dbus_send.contains("BatteryLevel"));
    }
    
    #[test]
    fn test_gdbus_format() {
        let cmd = sync::execute();
        let gdbus = cmd.to_gdbus();
        assert!(gdbus.starts_with("gdbus call"));
    }
}
