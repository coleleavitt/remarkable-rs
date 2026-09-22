//! Sync protocol traits for version-independent synchronization
//!
//! The reMarkable sync protocol has evolved through several versions:
//! - V1: REST-based document list (firmware 1.x)
//! - V1.5: Incremental sync support
//! - V2: Document versioning (firmware 2.x)
//! - V3: Hash-based Merkle tree (firmware 2.10+)
//! - V4: Enhanced conflict resolution (firmware 3.x)
//!
//! This module provides unified traits that work across all versions.

use std::collections::HashMap;

/// Sync protocol version
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ProtocolVersion {
    /// REST-based document list
    V1,
    /// Incremental sync
    V1_5,
    /// Document versioning
    V2,
    /// Hash-based Merkle tree
    V3,
    /// Enhanced conflict resolution
    V4,
}

impl ProtocolVersion {
    /// Whether this version uses hash-based sync
    pub fn is_hash_based(&self) -> bool {
        matches!(self, Self::V3 | Self::V4)
    }
    
    /// Whether this version supports incremental sync
    pub fn supports_incremental(&self) -> bool {
        !matches!(self, Self::V1)
    }
    
    /// Get version number
    pub fn number(&self) -> f32 {
        match self {
            Self::V1 => 1.0,
            Self::V1_5 => 1.5,
            Self::V2 => 2.0,
            Self::V3 => 3.0,
            Self::V4 => 4.0,
        }
    }
}

impl std::fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::V1 => write!(f, "v1"),
            Self::V1_5 => write!(f, "v1.5"),
            Self::V2 => write!(f, "v2"),
            Self::V3 => write!(f, "v3"),
            Self::V4 => write!(f, "v4"),
        }
    }
}

/// Sync root information
#[derive(Debug, Clone)]
pub struct SyncRoot {
    /// Root hash (for hash-based protocols)
    pub hash: String,
    /// Generation counter for optimistic locking
    pub generation: u64,
    /// Schema version
    pub schema_version: u32,
}

impl SyncRoot {
    pub fn new(hash: String, generation: u64) -> Self {
        Self {
            hash,
            generation,
            schema_version: 3,
        }
    }
}

/// Document entry in sync list
#[derive(Debug, Clone)]
pub struct SyncDoc {
    /// Document UUID
    pub id: String,
    /// Document type ("DocumentType" or "CollectionType")
    pub doc_type: String,
    /// Visible name
    pub name: Option<String>,
    /// Parent folder ID
    pub parent: Option<String>,
    /// Content hash
    pub hash: Option<String>,
    /// Version number
    pub version: u32,
    /// Last modified time
    pub modified: Option<String>,
    /// Whether deleted (soft delete)
    pub deleted: bool,
}

impl SyncDoc {
    pub fn is_folder(&self) -> bool {
        self.doc_type == "CollectionType"
    }
    
    pub fn is_document(&self) -> bool {
        self.doc_type == "DocumentType"
    }
}

/// Sync conflict information
#[derive(Debug, Clone)]
pub struct Conflict {
    /// Document ID
    pub document_id: String,
    /// Local version
    pub local_version: u32,
    /// Server version
    pub server_version: u32,
    /// Conflict description
    pub description: String,
}

/// Conflict resolution choice
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictChoice {
    /// Use local version
    Ours,
    /// Use server version
    Theirs,
    /// Merge both versions
    Merge,
    /// Create a copy (fork)
    Fork,
}

/// Sync error type
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("not authenticated")]
    NotAuthenticated,
    
    #[error("network error: {0}")]
    Network(String),
    
    #[error("document not found: {0}")]
    NotFound(String),
    
    #[error("conflict: {0}")]
    Conflict(String),
    
    #[error("version mismatch: expected {expected}, got {actual}")]
    VersionMismatch { expected: u64, actual: u64 },
    
    #[error("protocol error: {0}")]
    Protocol(String),
    
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Sync result summary
#[derive(Debug, Clone, Default)]
pub struct SyncResult {
    /// Downloaded document IDs
    pub downloaded: Vec<String>,
    /// Uploaded document IDs
    pub uploaded: Vec<String>,
    /// Deleted document IDs
    pub deleted: Vec<String>,
    /// New generation number
    pub generation: u64,
    /// Conflicts encountered
    pub conflicts: Vec<Conflict>,
}

impl SyncResult {
    pub fn had_conflicts(&self) -> bool {
        !self.conflicts.is_empty()
    }
    
    pub fn total_changes(&self) -> usize {
        self.downloaded.len() + self.uploaded.len() + self.deleted.len()
    }
}

/// Core sync provider trait
///
/// Implementations handle version-specific protocol details while
/// exposing a unified interface for sync operations.
///
/// # Associated Types
///
/// Implementations may customize root, document, and conflict types
/// while maintaining the core interface.
pub trait SyncProvider: Send + Sync {
    /// Get the protocol version
    fn version(&self) -> ProtocolVersion;
    
    /// Check if authenticated and ready
    fn is_ready(&self) -> bool;
    
    /// Get root hash/generation
    fn get_root(&self) -> impl std::future::Future<Output = Result<SyncRoot, SyncError>> + Send;
    
    /// List all documents
    fn list(&self) -> impl std::future::Future<Output = Result<Vec<SyncDoc>, SyncError>> + Send;
    
    /// Download a document
    fn download(&self, id: &str) -> impl std::future::Future<Output = Result<DownloadedDoc, SyncError>> + Send;
    
    /// Upload a document
    fn upload(&self, doc: &UploadDoc) -> impl std::future::Future<Output = Result<UploadResult, SyncError>> + Send;
    
    /// Delete a document
    fn delete(&self, id: &str) -> impl std::future::Future<Output = Result<(), SyncError>> + Send;
    
    /// Resolve a sync conflict
    fn resolve_conflict(
        &self,
        conflict: &Conflict,
        choice: ConflictChoice,
    ) -> impl std::future::Future<Output = Result<(), SyncError>> + Send;
    
    /// Perform full sync
    fn sync(&self) -> impl std::future::Future<Output = Result<SyncResult, SyncError>> + Send;
}

/// Downloaded document with all files
#[derive(Debug, Clone)]
pub struct DownloadedDoc {
    /// Document UUID
    pub id: String,
    /// Document metadata
    pub info: SyncDoc,
    /// Files by name (.metadata, .content, etc.)
    pub files: HashMap<String, Vec<u8>>,
    /// Page data by page ID
    pub pages: HashMap<String, Vec<u8>>,
}

impl DownloadedDoc {
    /// Get metadata file
    pub fn metadata(&self) -> Option<&Vec<u8>> {
        self.files.get(".metadata")
            .or_else(|| self.files.get("metadata"))
    }
    
    /// Get content file
    pub fn content(&self) -> Option<&Vec<u8>> {
        self.files.get(".content")
            .or_else(|| self.files.get("content"))
    }
}

/// Document to upload
#[derive(Debug, Clone)]
pub struct UploadDoc {
    /// Document UUID
    pub id: String,
    /// Parent folder ID
    pub parent: Option<String>,
    /// Visible name
    pub name: String,
    /// Files by name
    pub files: HashMap<String, Vec<u8>>,
    /// Page data by page ID
    pub pages: HashMap<String, Vec<u8>>,
}

/// Upload result
#[derive(Debug, Clone)]
pub struct UploadResult {
    /// Document ID
    pub id: String,
    /// Content hash (for hash-based protocols)
    pub hash: Option<String>,
    /// New version number
    pub version: u32,
}

/// Hash-based file operations (V3+)
pub trait HashSync: SyncProvider {
    /// Download a file by hash
    fn download_file(&self, hash: &str, filename: &str) -> impl std::future::Future<Output = Result<Vec<u8>, SyncError>> + Send;
    
    /// Upload a file and get hash
    fn upload_file(&self, data: &[u8], filename: &str) -> impl std::future::Future<Output = Result<String, SyncError>> + Send;
    
    /// Update root hash atomically
    fn update_root(&self, hash: &str, generation: u64) -> impl std::future::Future<Output = Result<(), SyncError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_protocol_version_ordering() {
        assert!(ProtocolVersion::V1 < ProtocolVersion::V3);
        assert!(ProtocolVersion::V3.is_hash_based());
        assert!(!ProtocolVersion::V2.is_hash_based());
    }
    
    #[test]
    fn test_sync_result() {
        let result = SyncResult {
            downloaded: vec!["a".to_string(), "b".to_string()],
            uploaded: vec!["c".to_string()],
            deleted: vec![],
            generation: 100,
            conflicts: vec![],
        };
        
        assert_eq!(result.total_changes(), 3);
        assert!(!result.had_conflicts());
    }
}
