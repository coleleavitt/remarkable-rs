//! Conflict resolution for sync operations
//!
//! Implements generation-based optimistic locking and conflict resolution
//! strategies for the reMarkable sync protocol.
//!
//! # Conflict Detection
//!
//! Conflicts are detected when:
//! 1. Local generation != server generation (concurrent modification)
//! 2. Hash mismatch between local and server versions
//! 3. Parent hash doesn't match expected value
//!
//! # Resolution Strategies
//!
//! - **Ours**: Keep local version, discard server changes
//! - **Theirs**: Keep server version, discard local changes  
//! - **Merge**: Combine changes (for CRDT-compatible operations)
//! - **Fork**: Create duplicate with conflict suffix

use crate::error::SyncError;
use crate::client::SyncClient;
use std::collections::HashMap;
use sha2::{Sha256, Digest};

/// Conflict resolution strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    /// Keep local version, discard server changes
    Ours,
    /// Keep server version, discard local changes
    Theirs,
    /// Attempt to merge changes (for compatible operations)
    Merge,
    /// Create a duplicate with conflict suffix
    Fork,
}

/// A detected conflict between local and server state
#[derive(Debug, Clone)]
pub struct Conflict {
    /// Document UUID
    pub doc_id: String,
    /// Local document hash
    pub local_hash: String,
    /// Server document hash
    pub server_hash: String,
    /// Local generation number
    pub local_gen: u64,
    /// Server generation number
    pub server_gen: u64,
    /// Type of conflict
    pub conflict_type: ConflictType,
}

/// Type of conflict detected
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictType {
    /// Both sides modified the document
    BothModified,
    /// Local deleted, server modified
    LocalDeletedServerModified,
    /// Local modified, server deleted
    LocalModifiedServerDeleted,
    /// Generation mismatch (concurrent sync)
    GenerationMismatch,
    /// Parent folder conflict
    ParentConflict,
}

/// Sync state tracking
#[derive(Debug, Clone)]
pub struct SyncState {
    /// Current root hash
    pub root_hash: String,
    /// Current generation number
    pub generation: u64,
    /// Document hashes (doc_id -> hash)
    pub doc_hashes: HashMap<String, String>,
    /// Previous sync state (for conflict detection)
    pub previous: Option<Box<SyncState>>,
}

impl SyncState {
    /// Create new sync state from root
    pub async fn from_server(client: &SyncClient) -> Result<Self, SyncError> {
        let root = client.get_root().await?;
        let docs = client.list_documents().await?;
        
        let doc_hashes: HashMap<String, String> = docs
            .iter()
            .map(|d| (d.uuid.clone(), d.hash.clone()))
            .collect();
        
        Ok(Self {
            root_hash: root.hash,
            generation: root.generation,
            doc_hashes,
            previous: None,
        })
    }
    
    /// Save current state as previous before sync
    pub fn checkpoint(&mut self) {
        let prev = Self {
            root_hash: self.root_hash.clone(),
            generation: self.generation,
            doc_hashes: self.doc_hashes.clone(),
            previous: None,
        };
        self.previous = Some(Box::new(prev));
    }
    
    /// Detect conflicts between local and server state
    pub fn detect_conflicts(&self, local_hashes: &HashMap<String, String>) -> Vec<Conflict> {
        let mut conflicts = Vec::new();
        
        // Check for documents modified on both sides
        for (doc_id, local_hash) in local_hashes {
            if let Some(server_hash) = self.doc_hashes.get(doc_id) {
                if local_hash != server_hash {
                    // Check if this was modified since last sync
                    let was_modified = self.previous
                        .as_ref()
                        .and_then(|p| p.doc_hashes.get(doc_id))
                        .map(|prev_hash| prev_hash != server_hash)
                        .unwrap_or(true);
                    
                    if was_modified {
                        conflicts.push(Conflict {
                            doc_id: doc_id.clone(),
                            local_hash: local_hash.clone(),
                            server_hash: server_hash.clone(),
                            local_gen: self.generation,
                            server_gen: self.generation,
                            conflict_type: ConflictType::BothModified,
                        });
                    }
                }
            }
        }
        
        // Check for documents deleted locally but modified on server
        if let Some(prev) = &self.previous {
            for (doc_id, prev_hash) in &prev.doc_hashes {
                let local_exists = local_hashes.contains_key(doc_id);
                let server_exists = self.doc_hashes.contains_key(doc_id);
                
                if !local_exists && server_exists {
                    let server_hash = self.doc_hashes.get(doc_id).unwrap();
                    if server_hash != prev_hash {
                        conflicts.push(Conflict {
                            doc_id: doc_id.clone(),
                            local_hash: String::new(),
                            server_hash: server_hash.clone(),
                            local_gen: self.generation,
                            server_gen: self.generation,
                            conflict_type: ConflictType::LocalDeletedServerModified,
                        });
                    }
                }
                
                if local_exists && !server_exists {
                    let local_hash = local_hashes.get(doc_id).unwrap();
                    if local_hash != prev_hash {
                        conflicts.push(Conflict {
                            doc_id: doc_id.clone(),
                            local_hash: local_hash.clone(),
                            server_hash: String::new(),
                            local_gen: self.generation,
                            server_gen: self.generation,
                            conflict_type: ConflictType::LocalModifiedServerDeleted,
                        });
                    }
                }
            }
        }
        
        conflicts
    }
}

/// Conflict resolver for handling sync conflicts
pub struct ConflictResolver {
    /// Default resolution strategy
    pub default_strategy: Resolution,
    /// Per-document strategy overrides
    pub overrides: HashMap<String, Resolution>,
}

impl ConflictResolver {
    /// Create a new conflict resolver with default strategy
    pub fn new(default_strategy: Resolution) -> Self {
        Self {
            default_strategy,
            overrides: HashMap::new(),
        }
    }
    
    /// Set resolution strategy for a specific document
    pub fn set_strategy(&mut self, doc_id: impl Into<String>, strategy: Resolution) {
        self.overrides.insert(doc_id.into(), strategy);
    }
    
    /// Get resolution strategy for a document
    pub fn get_strategy(&self, doc_id: &str) -> Resolution {
        self.overrides.get(doc_id).copied().unwrap_or(self.default_strategy)
    }
    
    /// Resolve a conflict according to configured strategy
    pub fn resolve(&self, conflict: &Conflict) -> ResolvedConflict {
        let strategy = self.get_strategy(&conflict.doc_id);
        
        match strategy {
            Resolution::Ours => ResolvedConflict {
                doc_id: conflict.doc_id.clone(),
                action: ResolveAction::KeepLocal,
                hash: conflict.local_hash.clone(),
            },
            Resolution::Theirs => ResolvedConflict {
                doc_id: conflict.doc_id.clone(),
                action: ResolveAction::KeepServer,
                hash: conflict.server_hash.clone(),
            },
            Resolution::Merge => {
                // For now, prefer server version in merge conflicts
                // Real CRDT merge would combine operations
                ResolvedConflict {
                    doc_id: conflict.doc_id.clone(),
                    action: ResolveAction::Merge,
                    hash: conflict.server_hash.clone(),
                }
            }
            Resolution::Fork => ResolvedConflict {
                doc_id: conflict.doc_id.clone(),
                action: ResolveAction::Fork,
                hash: conflict.local_hash.clone(),
            },
        }
    }
    
    /// Resolve all conflicts in a list
    pub fn resolve_all(&self, conflicts: &[Conflict]) -> Vec<ResolvedConflict> {
        conflicts.iter().map(|c| self.resolve(c)).collect()
    }
}

impl Default for ConflictResolver {
    fn default() -> Self {
        Self::new(Resolution::Theirs)
    }
}

/// Result of conflict resolution
#[derive(Debug, Clone)]
pub struct ResolvedConflict {
    /// Document UUID
    pub doc_id: String,
    /// Action to take
    pub action: ResolveAction,
    /// Hash to use
    pub hash: String,
}

/// Action to take for a resolved conflict
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveAction {
    /// Keep local version
    KeepLocal,
    /// Keep server version
    KeepServer,
    /// Merge both versions
    Merge,
    /// Create a fork (duplicate)
    Fork,
}

/// Generation-based optimistic locking
pub struct GenerationLock {
    /// Expected generation when lock was acquired
    expected_gen: u64,
    /// Hash at lock time
    locked_hash: String,
}

impl GenerationLock {
    /// Create a new generation lock
    pub fn new(generation: u64, hash: String) -> Self {
        Self {
            expected_gen: generation,
            locked_hash: hash,
        }
    }
    
    /// Check if the lock is still valid
    pub fn is_valid(&self, current_gen: u64, current_hash: &str) -> bool {
        self.expected_gen == current_gen && self.locked_hash == current_hash
    }
    
    /// Get the expected generation
    pub fn generation(&self) -> u64 {
        self.expected_gen
    }
    
    /// Get the next generation (for updates)
    pub fn next_generation(&self) -> u64 {
        self.expected_gen + 1
    }
}

/// Compute document hash from content
pub fn compute_document_hash(
    metadata: &[u8],
    content: &[u8],
    pages: &[(&str, &[u8])],
) -> String {
    let mut hasher = Sha256::new();
    
    // Hash metadata
    hasher.update(metadata);
    
    // Hash content
    hasher.update(content);
    
    // Hash pages in sorted order for determinism
    let mut sorted_pages: Vec<_> = pages.to_vec();
    sorted_pages.sort_by_key(|(id, _)| *id);
    
    for (page_id, page_data) in sorted_pages {
        hasher.update(page_id.as_bytes());
        hasher.update(page_data);
    }
    
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_conflict_resolver_default() {
        let resolver = ConflictResolver::default();
        assert_eq!(resolver.get_strategy("any-doc"), Resolution::Theirs);
    }
    
    #[test]
    fn test_conflict_resolver_override() {
        let mut resolver = ConflictResolver::new(Resolution::Theirs);
        resolver.set_strategy("doc-123", Resolution::Ours);
        
        assert_eq!(resolver.get_strategy("doc-123"), Resolution::Ours);
        assert_eq!(resolver.get_strategy("other-doc"), Resolution::Theirs);
    }
    
    #[test]
    fn test_generation_lock() {
        let lock = GenerationLock::new(5, "abc123".to_string());
        
        assert!(lock.is_valid(5, "abc123"));
        assert!(!lock.is_valid(6, "abc123"));
        assert!(!lock.is_valid(5, "different"));
        assert_eq!(lock.next_generation(), 6);
    }
    
    #[test]
    fn test_compute_document_hash() {
        let metadata = b"{\"name\": \"test\"}";
        let content = b"{\"pages\": []}";
        let pages: Vec<(&str, &[u8])> = vec![
            ("page1", b"data1"),
            ("page2", b"data2"),
        ];
        
        let hash = compute_document_hash(metadata, content, &pages);
        assert!(!hash.is_empty());
        assert_eq!(hash.len(), 64); // SHA-256 hex length
    }
}
