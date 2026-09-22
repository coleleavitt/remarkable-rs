//! .rm file writer

use remarkable_core::{Page, Stroke};
use crate::{LinesError, HEADER_V6, HEADER_SIZE};

/// Writer for .rm files
pub struct LinesWriter {
    data: Vec<u8>,
}

impl LinesWriter {
    /// Create a new writer
    pub fn new() -> Self {
        let mut data = vec![0u8; HEADER_SIZE];
        data[..HEADER_V6.len()].copy_from_slice(HEADER_V6);
        Self { data }
    }
    
    /// Add strokes to the file
    pub fn add_strokes(&mut self, strokes: &[Stroke]) {
        // TODO: Implement block writing
        for _stroke in strokes {
            // Write SceneLineItem block
        }
    }
    
    /// Get the raw data
    pub fn into_bytes(self) -> Vec<u8> {
        self.data
    }
    
    /// Write to file
    pub fn write_file(&self, path: &std::path::Path) -> Result<(), LinesError> {
        std::fs::write(path, &self.data)?;
        Ok(())
    }
}

impl Default for LinesWriter {
    fn default() -> Self {
        Self::new()
    }
}
