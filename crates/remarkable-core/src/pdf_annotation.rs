//! PDF annotation structures

use serde::{Deserialize, Serialize};

/// PDF page reference
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PdfPageRef {
    /// Page number (0-indexed)
    pub page: u32,
    /// Document ID
    pub document_id: String,
}

/// PDF annotation (stroke overlay on PDF page)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PdfAnnotation {
    /// Page the annotation is on
    pub page: u32,
    /// X position (relative to page)
    pub x: f32,
    /// Y position (relative to page)
    pub y: f32,
    /// Width
    pub width: f32,
    /// Height
    pub height: f32,
    /// Color (RGBA)
    #[serde(default)]
    pub color: Option<String>,
    /// Annotation type (highlight, underline, etc.)
    #[serde(rename = "type")]
    pub annotation_type: Option<String>,
}

/// PDF highlights layer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PdfHighlights {
    /// Highlights per page
    pub highlights: Vec<PdfHighlight>,
}

/// Single highlight
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PdfHighlight {
    /// Page number
    pub page: u32,
    /// Rectangles covered by this highlight
    pub rects: Vec<HighlightRect>,
    /// Color name
    #[serde(default)]
    pub color: Option<String>,
    /// Text content (if available)
    #[serde(default)]
    pub text: Option<String>,
}

/// Rectangle in highlight
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HighlightRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// EPUB annotation (anchor-based)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpubAnnotation {
    /// EPub CFI (Canonical Fragment Identifier)
    pub cfi: String,
    /// Anchor type
    #[serde(rename = "type")]
    pub annotation_type: String,
    /// Color
    #[serde(default)]
    pub color: Option<String>,
    /// Text content if highlight
    #[serde(default)]
    pub text: Option<String>,
    /// Note content if annotation
    #[serde(default)]
    pub note: Option<String>,
}

/// EPUB bookmarks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpubBookmark {
    /// EPub CFI
    pub cfi: String,
    /// Display text / title
    #[serde(default)]
    pub title: Option<String>,
    /// Creation time
    #[serde(default)]
    pub created: Option<String>,
}
