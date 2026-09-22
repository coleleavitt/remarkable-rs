//! Sync client for reMarkable cloud
//!
//! Provides document synchronization with reMarkable cloud services.
//!
//! # Features
//!
//! - Device pairing and token refresh
//! - Document listing and download
//! - Document creation, rename, move
//! - Folder creation
//! - Full hash-tree traversal
//!
//! # Example
//!
//! ```ignore
//! use remarkable_sync::SyncClient;
//!
//! let client = SyncClient::from_token_files("device.txt", "user.txt")?;
//! let docs = client.list_documents().await?;
//! ```

pub mod error;
pub mod client;
pub mod document_ops;
pub mod checksum;

pub use error::SyncError;
pub use client::*;
pub use document_ops::*;
