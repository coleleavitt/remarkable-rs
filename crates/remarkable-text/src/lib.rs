//! Text and CrdtTextItem support for reMarkable documents
//!
//! Handles typed text blocks in .rm files (firmware 3.0+)

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TextError {
    #[error("Parse error: {0}")]
    Parse(String),
    #[error("Invalid text block")]
    InvalidBlock,
}

/// Font used in text blocks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Font {
    /// RM Sans (default)
    RmSans,
    /// RM Serif
    RmSerif,
    /// RM Mono
    RmMono,
    /// RM Script
    RmScript,
    /// RM Sans Bold
    RmSansBold,
    /// RM Serif Bold
    RmSerifBold,
    /// RM Sans Italic
    RmSansItalic,
    /// RM Serif Italic
    RmSerifItalic,
}

impl Font {
    pub fn from_id(id: u32) -> Option<Self> {
        match id {
            0 => Some(Font::RmSans),
            1 => Some(Font::RmSerif),
            2 => Some(Font::RmMono),
            3 => Some(Font::RmScript),
            4 => Some(Font::RmSansBold),
            5 => Some(Font::RmSerifBold),
            6 => Some(Font::RmSansItalic),
            7 => Some(Font::RmSerifItalic),
            _ => None,
        }
    }
    
    pub fn to_id(&self) -> u32 {
        match self {
            Font::RmSans => 0,
            Font::RmSerif => 1,
            Font::RmMono => 2,
            Font::RmScript => 3,
            Font::RmSansBold => 4,
            Font::RmSerifBold => 5,
            Font::RmSansItalic => 6,
            Font::RmSerifItalic => 7,
        }
    }
    
    pub fn name(&self) -> &'static str {
        match self {
            Font::RmSans => "RM Sans",
            Font::RmSerif => "RM Serif",
            Font::RmMono => "RM Mono",
            Font::RmScript => "RM Script",
            Font::RmSansBold => "RM Sans Bold",
            Font::RmSerifBold => "RM Serif Bold",
            Font::RmSansItalic => "RM Sans Italic",
            Font::RmSerifItalic => "RM Serif Italic",
        }
    }
}

/// Text alignment
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

/// Text formatting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextFormat {
    pub font: Font,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
}

impl Default for TextFormat {
    fn default() -> Self {
        Self {
            font: Font::RmSans,
            size: 16.0,
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
        }
    }
}

/// A span of formatted text
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextSpan {
    pub text: String,
    pub format: TextFormat,
}

/// A paragraph of text
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextParagraph {
    pub spans: Vec<TextSpan>,
    pub align: TextAlign,
    pub line_height: f32,
}

impl Default for TextParagraph {
    fn default() -> Self {
        Self {
            spans: Vec::new(),
            align: TextAlign::Left,
            line_height: 1.5,
        }
    }
}

impl TextParagraph {
    pub fn new(text: &str) -> Self {
        Self {
            spans: vec![TextSpan {
                text: text.to_string(),
                format: TextFormat::default(),
            }],
            ..Default::default()
        }
    }
    
    pub fn plain_text(&self) -> String {
        self.spans.iter().map(|s| s.text.as_str()).collect()
    }
}

/// CrdtTextItem - a text block in a .rm file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrdtTextItem {
    /// Item ID (UUID)
    pub item_id: String,
    /// Parent item ID
    pub parent_id: String,
    /// Position on page (x, y)
    pub position: (f32, f32),
    /// Width of text block
    pub width: f32,
    /// Paragraphs
    pub paragraphs: Vec<TextParagraph>,
    /// CRDT timestamp (Lamport)
    pub timestamp: u64,
    /// Author ID
    pub author_id: u16,
}

impl CrdtTextItem {
    /// Create a new text item
    pub fn new(item_id: &str, position: (f32, f32), width: f32) -> Self {
        Self {
            item_id: item_id.to_string(),
            parent_id: String::new(),
            position,
            width,
            paragraphs: Vec::new(),
            timestamp: 0,
            author_id: 0,
        }
    }
    
    /// Add a paragraph
    pub fn add_paragraph(&mut self, text: &str) {
        self.paragraphs.push(TextParagraph::new(text));
    }
    
    /// Get all text as plain string
    pub fn plain_text(&self) -> String {
        self.paragraphs
            .iter()
            .map(|p| p.plain_text())
            .collect::<Vec<_>>()
            .join("\n")
    }
    
    /// Word count
    pub fn word_count(&self) -> usize {
        self.plain_text().split_whitespace().count()
    }
    
    /// Character count
    pub fn char_count(&self) -> usize {
        self.plain_text().chars().count()
    }
}

/// Parse CrdtTextItem from binary data
pub fn parse_text_item(data: &[u8]) -> Result<CrdtTextItem, TextError> {
    // Text items in v6 format use tagged blocks
    // This is a simplified parser - full implementation would parse CRDT blocks
    
    if data.len() < 16 {
        return Err(TextError::InvalidBlock);
    }
    
    // For now, return a placeholder
    Ok(CrdtTextItem {
        item_id: String::new(),
        parent_id: String::new(),
        position: (0.0, 0.0),
        width: 400.0,
        paragraphs: Vec::new(),
        timestamp: 0,
        author_id: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_font_roundtrip() {
        for id in 0..8 {
            let font = Font::from_id(id).unwrap();
            assert_eq!(font.to_id(), id);
        }
    }
    
    #[test]
    fn test_text_item() {
        let mut item = CrdtTextItem::new("test-id", (100.0, 200.0), 500.0);
        item.add_paragraph("Hello, world!");
        item.add_paragraph("Second paragraph.");
        
        assert_eq!(item.paragraphs.len(), 2);
        assert_eq!(item.word_count(), 4);
    }
    
    #[test]
    fn test_plain_text() {
        let mut item = CrdtTextItem::new("test", (0.0, 0.0), 100.0);
        item.add_paragraph("Line 1");
        item.add_paragraph("Line 2");
        
        assert_eq!(item.plain_text(), "Line 1\nLine 2");
    }
}
