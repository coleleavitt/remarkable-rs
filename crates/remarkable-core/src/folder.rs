//! Folder (Collection) type

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::document::{DocumentMetadata, DocumentType};

/// A folder containing documents
#[derive(Debug, Clone)]
pub struct Folder {
    /// Folder UUID
    pub id: Uuid,
    /// Metadata
    pub metadata: DocumentMetadata,
    /// Child document/folder IDs
    pub children: Vec<Uuid>,
}

impl Folder {
    /// Create a new empty folder
    pub fn new(name: &str) -> Self {
        let id = Uuid::new_v4();
        let now = chrono::Utc::now().timestamp_millis().to_string();
        
        Self {
            id,
            metadata: DocumentMetadata {
                created_time: now.clone(),
                last_modified: now,
                parent: String::new(),
                pinned: false,
                doc_type: DocumentType::Collection,
                visible_name: name.to_string(),
                version: None,
                deleted: None,
                last_opened: None,
                last_opened_page: None,
            },
            children: vec![],
        }
    }
    
    /// Create a folder with a parent
    pub fn new_with_parent(name: &str, parent: Uuid) -> Self {
        let mut folder = Self::new(name);
        folder.metadata.parent = parent.to_string();
        folder
    }
}
