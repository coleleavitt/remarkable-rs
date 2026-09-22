//! EPUB annotation parser for reMarkable documents
//!
//! Extracts and manages annotations from EPUB files.
//! Annotations use anchor-based storage that survives text reflow.

use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum EpubError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("ZIP error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Invalid EPUB format")]
    InvalidFormat,
    #[error("Annotation not found")]
    NotFound,
}

/// Annotation type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnnotationType {
    Highlight,
    Underline,
    Strikethrough,
    Note,
    Bookmark,
}

/// Text anchor for annotation positioning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextAnchor {
    /// EPUB spine item index
    pub spine_index: u32,
    /// Character offset from start of content
    pub char_offset: u32,
    /// Length in characters
    pub length: u32,
    /// CFI (EPUB Canonical Fragment Identifier)
    pub cfi: Option<String>,
}

/// Highlight color
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HighlightColor {
    Yellow,
    Blue,
    Green,
    Orange,
    Pink,
    Gray,
}

impl Default for HighlightColor {
    fn default() -> Self {
        Self::Yellow
    }
}

/// An annotation in an EPUB
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpubAnnotation {
    /// Unique ID
    pub id: String,
    /// Annotation type
    pub annotation_type: AnnotationType,
    /// Text anchor
    pub anchor: TextAnchor,
    /// Highlighted/selected text
    pub text: String,
    /// User note (for Note type)
    pub note: Option<String>,
    /// Highlight color
    pub color: HighlightColor,
    /// Creation timestamp (Unix ms)
    pub created_at: u64,
    /// Last modified timestamp
    pub modified_at: u64,
}

/// EPUB document with annotations
#[derive(Debug)]
pub struct EpubDocument {
    /// Document ID
    pub doc_id: String,
    /// EPUB title from metadata
    pub title: String,
    /// EPUB author from metadata
    pub author: String,
    /// All annotations
    pub annotations: Vec<EpubAnnotation>,
}

impl EpubDocument {
    /// Create new document
    pub fn new(doc_id: &str) -> Self {
        Self {
            doc_id: doc_id.to_string(),
            title: String::new(),
            author: String::new(),
            annotations: Vec::new(),
        }
    }
    
    /// Add a highlight
    pub fn add_highlight(
        &mut self,
        id: &str,
        anchor: TextAnchor,
        text: &str,
        color: HighlightColor,
    ) {
        self.annotations.push(EpubAnnotation {
            id: id.to_string(),
            annotation_type: AnnotationType::Highlight,
            anchor,
            text: text.to_string(),
            note: None,
            color,
            created_at: current_timestamp(),
            modified_at: current_timestamp(),
        });
    }
    
    /// Add a note
    pub fn add_note(
        &mut self,
        id: &str,
        anchor: TextAnchor,
        text: &str,
        note: &str,
    ) {
        self.annotations.push(EpubAnnotation {
            id: id.to_string(),
            annotation_type: AnnotationType::Note,
            anchor,
            text: text.to_string(),
            note: Some(note.to_string()),
            color: HighlightColor::Yellow,
            created_at: current_timestamp(),
            modified_at: current_timestamp(),
        });
    }
    
    /// Add a bookmark
    pub fn add_bookmark(&mut self, id: &str, anchor: TextAnchor) {
        self.annotations.push(EpubAnnotation {
            id: id.to_string(),
            annotation_type: AnnotationType::Bookmark,
            anchor,
            text: String::new(),
            note: None,
            color: HighlightColor::Yellow,
            created_at: current_timestamp(),
            modified_at: current_timestamp(),
        });
    }
    
    /// Get all highlights
    pub fn highlights(&self) -> Vec<&EpubAnnotation> {
        self.annotations
            .iter()
            .filter(|a| a.annotation_type == AnnotationType::Highlight)
            .collect()
    }
    
    /// Get all notes
    pub fn notes(&self) -> Vec<&EpubAnnotation> {
        self.annotations
            .iter()
            .filter(|a| a.annotation_type == AnnotationType::Note)
            .collect()
    }
    
    /// Get all bookmarks
    pub fn bookmarks(&self) -> Vec<&EpubAnnotation> {
        self.annotations
            .iter()
            .filter(|a| a.annotation_type == AnnotationType::Bookmark)
            .collect()
    }
    
    /// Export annotations to JSON
    pub fn to_json(&self) -> Result<String, EpubError> {
        Ok(serde_json::to_string_pretty(&self.annotations)?)
    }
    
    /// Import annotations from JSON
    pub fn from_json(doc_id: &str, json: &str) -> Result<Self, EpubError> {
        let annotations: Vec<EpubAnnotation> = serde_json::from_str(json)?;
        Ok(Self {
            doc_id: doc_id.to_string(),
            title: String::new(),
            author: String::new(),
            annotations,
        })
    }
}

/// Parse EPUB metadata
pub fn parse_epub_metadata(path: &Path) -> Result<(String, String), EpubError> {
    let file = std::fs::File::open(path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    
    // Find container.xml to get OPF path
    let mut container = archive.by_name("META-INF/container.xml")?;
    let mut container_xml = String::new();
    container.read_to_string(&mut container_xml)?;
    
    // Simple extraction - in production use proper XML parser
    let title = extract_tag(&container_xml, "dc:title").unwrap_or_default();
    let author = extract_tag(&container_xml, "dc:creator").unwrap_or_default();
    
    Ok((title, author))
}

fn extract_tag(xml: &str, tag: &str) -> Option<String> {
    let start = format!("<{}>", tag);
    let end = format!("</{}>", tag);
    
    let start_idx = xml.find(&start)? + start.len();
    let end_idx = xml[start_idx..].find(&end)? + start_idx;
    
    Some(xml[start_idx..end_idx].to_string())
}

fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_document_creation() {
        let doc = EpubDocument::new("test-doc");
        assert_eq!(doc.doc_id, "test-doc");
        assert!(doc.annotations.is_empty());
    }
    
    #[test]
    fn test_add_highlight() {
        let mut doc = EpubDocument::new("test");
        doc.add_highlight(
            "h1",
            TextAnchor { spine_index: 0, char_offset: 100, length: 50, cfi: None },
            "Highlighted text",
            HighlightColor::Yellow,
        );
        
        assert_eq!(doc.highlights().len(), 1);
        assert_eq!(doc.highlights()[0].text, "Highlighted text");
    }
    
    #[test]
    fn test_json_roundtrip() {
        let mut doc = EpubDocument::new("test");
        doc.add_highlight(
            "h1",
            TextAnchor { spine_index: 0, char_offset: 0, length: 10, cfi: None },
            "Test",
            HighlightColor::Blue,
        );
        
        let json = doc.to_json().unwrap();
        let doc2 = EpubDocument::from_json("test", &json).unwrap();
        assert_eq!(doc2.annotations.len(), 1);
    }
}
