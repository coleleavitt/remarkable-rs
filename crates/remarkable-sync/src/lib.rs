//! Sync client for reMarkable cloud
//!
//! Provides document synchronization with reMarkable cloud services.
//!
//! # Features
//!
//! - Device pairing and token refresh
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
//! use remarkable_sync::{SyncClient, Resolution, ConflictResolver};
//!
//! // Create client from tokens
//! let client = SyncClient::from_token_files("device.txt", "user.txt")?;
//!
//! // List documents
//! let docs = client.list_documents().await?;
//!
//! // Create a notebook
//! let doc_id = create_notebook(&client, "My Notebook", None).await?;
//!
//! // Download with conflict handling
//! let resolver = ConflictResolver::new(Resolution::Theirs);
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

pub use error::SyncError;
pub use client::*;
pub use document_ops::*;
pub use conflict::*;
pub use merkle::*;
