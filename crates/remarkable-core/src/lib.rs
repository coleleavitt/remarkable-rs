//! Core types and traits for reMarkable documents
//!
//! This crate provides the fundamental data structures used across
//! the remarkable-* crate ecosystem.

mod types;

pub mod crdt;
pub mod template;
pub mod pdf_annotation;

// Export all types from the unified types module
pub use types::*;
pub use template::*;
pub use pdf_annotation::*;

/// Common result type
pub type Result<T> = std::result::Result<T, Error>;

/// Common error type
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Parse error: {0}")]
    Parse(String),
    
    #[error("Not found: {0}")]
    NotFound(String),
}
