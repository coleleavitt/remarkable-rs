//! Version-specific .rm file parsers and writers
//!
//! The reMarkable .rm format has evolved through several versions:
//! - v3 (firmware 1.x-2.5): Flat structure, no layers
//! - v5 (firmware 2.6-2.x): Layer support with transforms
//! - v6 (firmware 3.0+): CRDT-based with tagged blocks

pub mod v3;
pub mod v5;
pub mod v6;
pub mod writer_v3;
pub mod writer_v5;

pub use v3::V3Parser;
pub use v5::V5Parser;
pub use v6::V6Parser;
pub use writer_v3::V3Writer;
pub use writer_v5::V5Writer;

/// Header size for all versions
pub const HEADER_SIZE: usize = 43;

/// Version 3 header bytes
pub const HEADER_V3: &[u8; HEADER_SIZE] = b"reMarkable .lines file, version=3          ";

/// Version 5 header bytes  
pub const HEADER_V5: &[u8; HEADER_SIZE] = b"reMarkable .lines file, version=5          ";

/// Version 6 header bytes
pub const HEADER_V6: &[u8; HEADER_SIZE] = b"reMarkable .lines file, version=6          ";

use remarkable_core::Stroke;
use crate::LinesError;

/// Trait for version-specific parsers
pub trait RmParser {
    /// Parse all strokes from the file
    fn parse_strokes(&mut self) -> Result<Vec<Stroke>, LinesError>;
    
    /// Get the format version
    fn version(&self) -> u32;
    
    /// Get the point size in bytes for this version
    fn point_size(&self) -> usize;
}

/// Detect the version from file data
pub fn detect_version(data: &[u8]) -> Option<u32> {
    if data.len() < HEADER_SIZE {
        return None;
    }
    
    if &data[..HEADER_SIZE] == HEADER_V3 {
        Some(3)
    } else if &data[..HEADER_SIZE] == HEADER_V5 {
        Some(5)
    } else if &data[..HEADER_SIZE] == HEADER_V6 {
        Some(6)
    } else {
        None
    }
}

/// Create a parser for the detected version
pub fn create_parser(data: Vec<u8>) -> Result<Box<dyn RmParser>, LinesError> {
    let version = detect_version(&data).ok_or(LinesError::InvalidHeader)?;
    
    match version {
        3 => Ok(Box::new(V3Parser::new(data)?)),
        5 => Ok(Box::new(V5Parser::new(data)?)),
        6 => Ok(Box::new(V6Parser::new(data)?)),
        _ => Err(LinesError::UnsupportedVersion(version)),
    }
}

/// Write strokes to specified version format
pub fn write_rm(strokes: &[Stroke], version: u32) -> Result<Vec<u8>, LinesError> {
    match version {
        3 => V3Writer::write(strokes),
        5 => V5Writer::write(strokes),
        6 => crate::writer_v6::V6Writer::new().write_strokes(strokes),
        _ => Err(LinesError::UnsupportedVersion(version)),
    }
}
