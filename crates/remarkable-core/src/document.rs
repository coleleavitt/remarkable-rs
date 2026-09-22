//! Document types and metadata

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};
use crate::crdt::CrdtValue;
use crate::page::Page;

/// Document type identifier
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DocumentType {
    #[serde(rename = "DocumentType")]
    Document,
    #[serde(rename = "CollectionType")]
    Collection,
}

/// Document metadata (.metadata file)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentMetadata {
    /// Creation timestamp (milliseconds since epoch)
    pub created_time: String,
    /// Last modification timestamp
    pub last_modified: String,
    /// Parent folder UUID (empty string for root)
    pub parent: String,
    /// Whether document is pinned
    pub pinned: bool,
    /// Document type
    #[serde(rename = "type")]
    pub doc_type: DocumentType,
    /// Display name
    pub visible_name: String,
    /// Version (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
    /// Deleted flag
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted: Option<bool>,
    /// Last opened timestamp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_opened: Option<String>,
    /// Last opened page index
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_opened_page: Option<u32>,
}

/// Document content with CRDT pages (.content file)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentContent {
    /// CRDT-enabled pages
    #[serde(rename = "cPages", skip_serializing_if = "Option::is_none")]
    pub c_pages: Option<CrdtPages>,
    /// Legacy pages (non-CRDT)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pages: Option<Vec<String>>,
    /// Cover page number (-1 for none)
    #[serde(default)]
    pub cover_page_number: i32,
    /// File type (notebook, pdf, epub)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_type: Option<String>,
    /// Format version
    #[serde(default)]
    pub format_version: u32,
    /// Page orientation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orientation: Option<String>,
    /// Total page count
    #[serde(default)]
    pub page_count: u32,
    /// Extra metadata (pen settings, etc)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra_metadata: Option<serde_json::Value>,
}

/// CRDT-enabled page list
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrdtPages {
    /// Page entries with CRDT timestamps
    pub pages: Vec<CrdtPage>,
    /// Last opened page
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_opened: Option<CrdtValue<String>>,
    /// Original page index
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original: Option<CrdtValue<i32>>,
    /// UUID mappings
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uuids: Option<Vec<UuidMapping>>,
}

/// A page entry with CRDT metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrdtPage {
    /// Page UUID
    pub id: String,
    /// Page index with timestamp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idx: Option<CrdtValue<String>>,
    /// Template name with timestamp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template: Option<CrdtValue<String>>,
    /// Vertical scroll position
    #[serde(rename = "verticalScroll", skip_serializing_if = "Option::is_none")]
    pub vertical_scroll: Option<CrdtValue<i32>>,
    /// Last scroll time
    #[serde(rename = "scrollTime", skip_serializing_if = "Option::is_none")]
    pub scroll_time: Option<CrdtValue<String>>,
}

/// UUID to replica mapping
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UuidMapping {
    pub first: String,
    pub second: u32,
}

/// A complete document with all components
#[derive(Debug, Clone)]
pub struct Document {
    /// Document UUID
    pub id: Uuid,
    /// Metadata
    pub metadata: DocumentMetadata,
    /// Content structure
    pub content: DocumentContent,
    /// Pages with stroke data
    pub pages: Vec<Page>,
    /// Original PDF data (if imported)
    pub pdf_data: Option<Vec<u8>>,
}

impl Document {
    /// Create a new empty notebook
    pub fn new_notebook(name: &str) -> Self {
        let id = Uuid::new_v4();
        let now = chrono::Utc::now().timestamp_millis().to_string();
        
        Self {
            id,
            metadata: DocumentMetadata {
                created_time: now.clone(),
                last_modified: now,
                parent: String::new(),
                pinned: false,
                doc_type: DocumentType::Document,
                visible_name: name.to_string(),
                version: None,
                deleted: None,
                last_opened: None,
                last_opened_page: None,
            },
            content: DocumentContent {
                c_pages: Some(CrdtPages {
                    pages: vec![],
                    last_opened: None,
                    original: None,
                    uuids: None,
                }),
                pages: None,
                cover_page_number: -1,
                file_type: Some("notebook".to_string()),
                format_version: 2,
                orientation: Some("portrait".to_string()),
                page_count: 0,
                extra_metadata: None,
            },
            pages: vec![],
            pdf_data: None,
        }
    }
    
    /// Check if this is a folder
    pub fn is_folder(&self) -> bool {
        self.metadata.doc_type == DocumentType::Collection
    }
}
