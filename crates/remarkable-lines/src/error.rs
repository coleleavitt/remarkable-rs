//! Error types for lines parsing

use thiserror::Error;

#[derive(Error, Debug)]
pub enum LinesError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Invalid header: expected 'reMarkable .lines file, version=N'")]
    InvalidHeader,
    
    #[error("Unsupported version: {0}")]
    UnsupportedVersion(u32),
    
    #[error("Parse error at offset {offset}: {message}")]
    Parse { offset: usize, message: String },
    
    #[error("Invalid block type: {0}")]
    InvalidBlockType(u8),
    
    #[error("Unexpected end of data")]
    UnexpectedEof,
}
