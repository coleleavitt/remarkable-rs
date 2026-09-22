//! Merkle tree operations for sync

use sha2::{Sha256, Digest};
use remarkable_core::Document;

/// Compute hash of a document
pub fn hash_document(doc: &Document) -> String {
    let mut hasher = Sha256::new();
    
    // Hash document ID
    hasher.update(doc.id.as_bytes());
    
    // Hash metadata
    if let Ok(meta_json) = serde_json::to_string(&doc.metadata) {
        hasher.update(meta_json.as_bytes());
    }
    
    // Hash content
    if let Ok(content_json) = serde_json::to_string(&doc.content) {
        hasher.update(content_json.as_bytes());
    }
    
    hex::encode(hasher.finalize())
}

/// Compute Merkle root of document list
pub fn compute_root(documents: &[Document]) -> String {
    if documents.is_empty() {
        return String::new();
    }
    
    let mut hashes: Vec<String> = documents
        .iter()
        .map(hash_document)
        .collect();
    
    // Sort for deterministic ordering
    hashes.sort();
    
    // Build Merkle tree
    while hashes.len() > 1 {
        let mut next_level = Vec::new();
        for chunk in hashes.chunks(2) {
            let mut hasher = Sha256::new();
            hasher.update(chunk[0].as_bytes());
            if chunk.len() > 1 {
                hasher.update(chunk[1].as_bytes());
            }
            next_level.push(hex::encode(hasher.finalize()));
        }
        hashes = next_level;
    }
    
    hashes.pop().unwrap_or_default()
}

/// Sync generation (monotonic counter)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Generation(pub u64);

impl Generation {
    pub fn new(value: u64) -> Self {
        Self(value)
    }
    
    pub fn next(&self) -> Self {
        Self(self.0 + 1)
    }
}
