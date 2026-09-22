//! Error types for sync operations

use thiserror::Error;

/// Sync operation errors
#[derive(Error, Debug)]
pub enum SyncError {
    /// Authentication required
    #[error("Authentication required")]
    AuthRequired,
    
    /// Authentication failed
    #[error("Authentication failed: {0}")]
    Auth(String),
    
    /// Token expired
    #[error("Token expired")]
    TokenExpired,
    
    /// Invalid token
    #[error("Invalid token: {0}")]
    InvalidToken(String),
    
    /// Document/resource not found
    #[error("Not found: {0}")]
    NotFound(String),
    
    /// Rate limited by server
    #[error("Rate limited")]
    RateLimited,
    
    /// Server error
    #[error("Server error: {status} - {message}")]
    Server { status: u16, message: String },
    
    /// HTTP request error
    #[error("Request error: {0}")]
    Request(#[from] reqwest::Error),
    
    /// JSON parse error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    
    /// Missing rm-filename header
    #[error("Missing rm-filename header")]
    MissingFilename,
    
    /// Parse error
    #[error("Parse error: {0}")]
    Parse(String),
    
    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    
    /// Sync conflict
    #[error("Sync conflict: local={local_gen}, remote={remote_gen}")]
    Conflict { local_gen: u64, remote_gen: u64 },
}
