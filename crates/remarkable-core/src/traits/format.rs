//! Document format traits for version-independent parsing
//!
//! The reMarkable .rm format has evolved through several versions:
//! - v3: Flat structure (firmware 1.x-2.5)
//! - v5: Layered with transforms (firmware 2.6-2.x)  
//! - v6: CRDT-based tagged blocks (firmware 3.0+)
//!
//! This trait system allows version-agnostic document handling.

use std::fmt;

use crate::{Layer, Stroke};

/// Format version discriminant
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FormatVersion {
    /// Legacy flat format (firmware 1.x - 2.5)
    V3,
    /// Layered format (firmware 2.6 - 2.x)
    V5,
    /// CRDT-based tagged blocks (firmware 3.0+)
    V6,
}

impl FormatVersion {
    /// Header bytes for this version
    pub const fn header(&self) -> &'static [u8; 43] {
        match self {
            Self::V3 => b"reMarkable .lines file, version=3          ",
            Self::V5 => b"reMarkable .lines file, version=5          ",
            Self::V6 => b"reMarkable .lines file, version=6          ",
        }
    }
    
    /// Point size in bytes for this version
    pub const fn point_size(&self) -> usize {
        match self {
            Self::V3 | Self::V5 => 24, // 6 floats
            Self::V6 => 14,            // compressed
        }
    }
    
    /// Detect version from header bytes
    pub fn detect(data: &[u8]) -> Option<Self> {
        if data.len() < 43 {
            return None;
        }
        let header = &data[..43];
        if header == Self::V3.header() {
            Some(Self::V3)
        } else if header == Self::V5.header() {
            Some(Self::V5)
        } else if header == Self::V6.header() {
            Some(Self::V6)
        } else {
            None
        }
    }
}

impl fmt::Display for FormatVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::V3 => write!(f, "v3"),
            Self::V5 => write!(f, "v5"),
            Self::V6 => write!(f, "v6"),
        }
    }
}

/// Error type for format operations
#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error("invalid header: expected version header")]
    InvalidHeader,
    
    #[error("unsupported version: {0}")]
    UnsupportedVersion(String),
    
    #[error("unexpected end of file at offset {0}")]
    UnexpectedEof(usize),
    
    #[error("invalid data: {0}")]
    InvalidData(String),
    
    #[error("serialization failed: {0}")]
    SerializationFailed(String),
}

/// Result of parsing a document format
pub struct ParsedDocument {
    /// Format version detected
    pub version: FormatVersion,
    /// Extracted strokes
    pub strokes: Vec<Stroke>,
    /// Extracted layers (v5+)
    pub layers: Vec<Layer>,
    /// Raw block data (v6 only)
    pub blocks: Vec<BlockData>,
}

/// Block data from v6 format
#[derive(Debug, Clone)]
pub struct BlockData {
    /// Block type identifier
    pub block_type: u32,
    /// Block offset in file
    pub offset: usize,
    /// Block length
    pub length: usize,
    /// Raw block bytes
    pub data: Vec<u8>,
}

impl ParsedDocument {
    /// Create an empty parsed document
    pub fn empty(version: FormatVersion) -> Self {
        Self {
            version,
            strokes: Vec::new(),
            layers: Vec::new(),
            blocks: Vec::new(),
        }
    }
    
    /// Get all strokes (flattening layers if present)
    pub fn all_strokes(&self) -> Vec<&Stroke> {
        if self.layers.is_empty() {
            self.strokes.iter().collect()
        } else {
            self.layers.iter().flat_map(|l| &l.strokes).collect()
        }
    }
}

/// Core trait for document format parsers and serializers
///
/// Implementations provide version-specific parsing while exposing
/// a unified interface for document access.
///
/// # Object Safety
///
/// This trait is object-safe for parsing operations. Use
/// `ParsedDocument` for format-agnostic document handling.
///
/// # Example
///
/// ```ignore
/// use remarkable_core::traits::{DocumentFormat, FormatVersion};
///
/// let data = std::fs::read("page.rm")?;
/// let version = FormatVersion::detect(&data).ok_or("unknown format")?;
/// let parser = match version {
///     FormatVersion::V6 => V6Format::new(),
///     // ...
/// };
/// let doc = parser.parse(&data)?;
/// ```
pub trait DocumentFormat: Send + Sync {
    /// The format version this implementation handles
    fn version(&self) -> FormatVersion;
    
    /// Magic bytes / header for this format
    fn magic(&self) -> &'static [u8] {
        self.version().header()
    }
    
    /// Parse raw bytes into a document
    fn parse(&self, data: &[u8]) -> Result<ParsedDocument, FormatError>;
    
    /// Serialize a document back to bytes
    fn serialize(&self, doc: &ParsedDocument) -> Result<Vec<u8>, FormatError>;
    
    /// Extract strokes only (convenience method)
    fn parse_strokes(&self, data: &[u8]) -> Result<Vec<Stroke>, FormatError> {
        self.parse(data).map(|d| d.strokes)
    }
    
    /// Check if data matches this format
    fn matches(&self, data: &[u8]) -> bool {
        data.len() >= 43 && &data[..43] == self.magic()
    }
}

/// Factory for creating format handlers
pub struct FormatFactory;

impl FormatFactory {
    /// Detect version and suggest appropriate parser
    pub fn detect_version(data: &[u8]) -> Option<FormatVersion> {
        FormatVersion::detect(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_format_version_detection() {
        let v3_header = b"reMarkable .lines file, version=3          ";
        let v5_header = b"reMarkable .lines file, version=5          ";
        let v6_header = b"reMarkable .lines file, version=6          ";
        
        assert_eq!(FormatVersion::detect(v3_header), Some(FormatVersion::V3));
        assert_eq!(FormatVersion::detect(v5_header), Some(FormatVersion::V5));
        assert_eq!(FormatVersion::detect(v6_header), Some(FormatVersion::V6));
        assert_eq!(FormatVersion::detect(b"invalid"), None);
    }
    
    #[test]
    fn test_point_sizes() {
        assert_eq!(FormatVersion::V3.point_size(), 24);
        assert_eq!(FormatVersion::V5.point_size(), 24);
        assert_eq!(FormatVersion::V6.point_size(), 14);
    }
}
