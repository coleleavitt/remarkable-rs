//! Sync protocol version definitions

use std::fmt;

/// Supported sync protocol versions
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SyncVersion {
    /// Original document-storage JSON API (firmware 1.x-2.x)
    /// 
    /// Endpoints:
    /// - GET /document-storage/json/2/docs
    /// - PUT /document-storage/json/2/upload/request
    /// - DELETE /document-storage/json/2/delete
    V1,
    
    /// Transitional protocol with batch operations (DocumentSync_1_5)
    /// 
    /// Similar to V1 but with sync state tracking and incremental updates.
    V1_5,
    
    /// Batch sync protocol (firmware 2.x-3.x)
    /// 
    /// Endpoints:
    /// - GET /sync/v2/root
    /// - GET /sync/v2/sync-complete
    /// - POST /sync/v2/batch
    /// - WebSocket notifications
    V2,
    
    /// Current hash-tree based protocol (tectonic)
    /// 
    /// Endpoints:
    /// - GET /sync/v3/root
    /// - GET /sync/v3/files/{hash}
    /// - PUT /sync/v3/files/{hash}
    /// - Requires rm-filename header
    V3,
    
    /// Merkle tree with generation counters (beta firmware)
    /// 
    /// Endpoints:
    /// - GET /sync/v4/root
    /// - Optimistic locking with generations
    /// - Delta sync support
    V4,
}

impl SyncVersion {
    /// Get all versions in order from oldest to newest
    pub fn all() -> &'static [SyncVersion] {
        &[
            SyncVersion::V1,
            SyncVersion::V1_5,
            SyncVersion::V2,
            SyncVersion::V3,
            SyncVersion::V4,
        ]
    }
    
    /// Get the version string used in API paths
    pub fn path_version(&self) -> &'static str {
        match self {
            SyncVersion::V1 => "json/2",
            SyncVersion::V1_5 => "json/2",
            SyncVersion::V2 => "v2",
            SyncVersion::V3 => "v3",
            SyncVersion::V4 => "v4",
        }
    }
    
    /// Whether this version uses hash-based content addressing
    pub fn uses_hashes(&self) -> bool {
        matches!(self, SyncVersion::V2 | SyncVersion::V3 | SyncVersion::V4)
    }
    
    /// Whether this version requires the rm-filename header
    pub fn requires_rm_filename(&self) -> bool {
        matches!(self, SyncVersion::V3 | SyncVersion::V4)
    }
    
    /// Whether this version supports batch operations
    pub fn supports_batch(&self) -> bool {
        matches!(self, SyncVersion::V1_5 | SyncVersion::V2 | SyncVersion::V3 | SyncVersion::V4)
    }
    
    /// Whether this version supports generation-based optimistic locking
    pub fn supports_generations(&self) -> bool {
        matches!(self, SyncVersion::V3 | SyncVersion::V4)
    }
    
    /// Whether this version supports delta sync
    pub fn supports_delta_sync(&self) -> bool {
        matches!(self, SyncVersion::V4)
    }
    
    /// Minimum firmware version that supports this protocol
    pub fn min_firmware(&self) -> &'static str {
        match self {
            SyncVersion::V1 => "1.0.0",
            SyncVersion::V1_5 => "2.0.0",
            SyncVersion::V2 => "2.5.0",
            SyncVersion::V3 => "3.0.0",
            SyncVersion::V4 => "3.28.0",
        }
    }
}

impl fmt::Display for SyncVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SyncVersion::V1 => write!(f, "v1"),
            SyncVersion::V1_5 => write!(f, "v1.5"),
            SyncVersion::V2 => write!(f, "v2"),
            SyncVersion::V3 => write!(f, "v3"),
            SyncVersion::V4 => write!(f, "v4"),
        }
    }
}

impl Default for SyncVersion {
    fn default() -> Self {
        SyncVersion::V3
    }
}

/// Parse version from string
impl std::str::FromStr for SyncVersion {
    type Err = String;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "v1" | "1" => Ok(SyncVersion::V1),
            "v1.5" | "1.5" => Ok(SyncVersion::V1_5),
            "v2" | "2" => Ok(SyncVersion::V2),
            "v3" | "3" => Ok(SyncVersion::V3),
            "v4" | "4" => Ok(SyncVersion::V4),
            _ => Err(format!("Unknown sync version: {}", s)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_version_ordering() {
        assert!(SyncVersion::V1 < SyncVersion::V1_5);
        assert!(SyncVersion::V1_5 < SyncVersion::V2);
        assert!(SyncVersion::V2 < SyncVersion::V3);
        assert!(SyncVersion::V3 < SyncVersion::V4);
    }
    
    #[test]
    fn test_version_parse() {
        assert_eq!("v3".parse::<SyncVersion>().unwrap(), SyncVersion::V3);
        assert_eq!("v1.5".parse::<SyncVersion>().unwrap(), SyncVersion::V1_5);
    }
}
