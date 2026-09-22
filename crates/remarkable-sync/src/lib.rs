//! Sync client for reMarkable cloud and local servers
//!
//! Provides document synchronization with reMarkable cloud services
//! or local remarkable-server instances.
//!
//! # Features
//!
//! - Device pairing and token refresh (cloud and local)
//! - Auto-detect server type
//! - Document listing and download
//! - Document creation, rename, move, delete
//! - Folder creation and management
//! - Full hash-tree traversal
//! - Conflict resolution with generation tracking
//! - CRC32C checksum verification
//!
//! # Example
//!
//! ```ignore
//! use remarkable_sync::{SyncClient, Resolution, ConflictResolver, ServerConfig};
//!
//! // Create client from tokens (auto-detects cloud)
//! let client = SyncClient::from_token_files("device.txt", "user.txt")?;
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

pub mod error;
pub mod client;
pub mod document_ops;
pub mod checksum;
pub mod conflict;
pub mod merkle;
pub mod local;

pub use error::SyncError;
pub use client::*;
pub use document_ops::*;
pub use conflict::*;
pub use merkle::*;
pub use local::{LocalServerConfig, LocalServerClient, LocalTokens, ServerConfig, ServerType};
