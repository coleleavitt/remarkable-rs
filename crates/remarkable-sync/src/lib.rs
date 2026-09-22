//! Sync client for reMarkable cloud and local servers
//!
//! Provides document synchronization with reMarkable cloud services
//! or local remarkable-server instances.
//!
//! # Features
//!
//! - Multi-version protocol support (V1-V4)
//! - Automatic protocol detection
//! - Device pairing and token refresh (cloud and local)
//! - Auto-detect server type
//! - Document listing and download
//! - Document creation, rename, move, delete
//! - Folder creation and management
//! - Full hash-tree traversal
//! - Conflict resolution with generation tracking
//! - CRC32C checksum verification
//! - Delta sync support (V4)
//!
//! # Protocol Versions
//!
//! | Version | Firmware | Description |
//! |---------|----------|-------------|
//! | V1 | 1.x-2.x | Original document-storage JSON API |
//! | V1.5 | 2.x | Transitional with batch operations |
//! | V2 | 2.x-3.x | Batch sync with signed URLs |
//! | V3 | 3.x | Hash-tree based (current production) |
//! | V4 | 3.28+ | Merkle tree with generation counters |
//!
//! # Example
//!
//! ```ignore
//! use remarkable_sync::{SyncClient, Resolution, ConflictResolver, ServerConfig};
//! use remarkable_sync::protocol::{UnifiedSyncClient, SyncConfig, SyncVersion};
//!
//! // Create client from tokens (auto-detects cloud and protocol version)
//! let client = SyncClient::from_token_files("device.txt", "user.txt")?;
//!
//! // Or use the unified client with explicit version
//! let config = SyncConfig::cloud()
//!     .with_user_token(token)
//!     .with_region("eu");
//! let client = UnifiedSyncClient::with_version(config, SyncVersion::V3)?;
//!
//! // Auto-detect protocol version
//! let client = UnifiedSyncClient::new(config).await?;
//!
//! // Or create client for local server
//! let client = SyncClient::new()
//!     .with_server(ServerConfig::local("http://localhost:8080"));
//!
//! // Pair with local server
//! use remarkable_sync::local::{LocalServerClient, LocalServerConfig};
//! let config = LocalServerConfig::new("http://localhost:8080");
//! let local = LocalServerClient::new(config)?;
//! let tokens = local.exchange_code("12345678", "device-id").await?;
//! ```
//!
//! # Upload Protocol
//!
//! Uploads require several headers for the reMarkable cloud API:
//! - `rm-filename`: File identifier (filename only, no path)
//! - `x-goog-hash`: CRC32C checksum in base64 (`crc32c={base64}`)
//! - `rm-parent-hash`: Parent document hash for tree linkage
//! - `rm-sync-id`: Unique sync session identifier
//! - `rm-batch-number`: Batch operation counter
//!
//! # Sync Protocol
//!
//! The sync uses a Merkle tree structure:
//! ```text
//! Root (from /sync/v3/root)
//!   └── Root index (documents.json)
//!         └── Document hashes
//!               └── Document schema (schema.txt)
//!                     └── File hashes (.content, .metadata, .rm)
//! ```

pub mod auth;
pub mod error;
pub mod client;
pub mod document_ops;
pub mod checksum;
pub mod conflict;
pub mod merkle;
pub mod local;
pub mod protocol;

pub use error::SyncError;
pub use client::*;
pub use document_ops::*;
pub use conflict::*;
pub use merkle::*;
pub use local::{LocalServerConfig, LocalServerClient, LocalTokens, ServerConfig, ServerType};

// Re-export protocol types for convenience
pub use protocol::{
    SyncVersion, SyncProtocol, SyncConfig, RootInfo, DocumentInfo, Document,
    SyncResult, UploadResult, UnifiedSyncClient, UnifiedClientBuilder,
    detect_protocol, version_from_firmware,
};
