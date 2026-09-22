//! PDF annotation overlay for reMarkable documents
//!
//! This crate provides functionality to overlay reMarkable .rm strokes
//! onto PDF pages, creating annotated PDF exports.
//!
//! # Example
//!
//! ```ignore
//! use remarkable_pdf::PdfAnnotator;
//!
//! let mut annotator = PdfAnnotator::open("document.pdf")?;
//! annotator.add_strokes(0, &strokes)?;  // Add strokes to page 0
//! annotator.save("annotated.pdf")?;
//! ```

use std::path::Path;
use lopdf::{Document, Object, Stream, Dictionary, content::{Content, Operation}};
use remarkable_core::Stroke;
use remarkable_lines::parse_rm_file;
use thiserror::Error;

/// Errors for PDF operations
#[derive(Error, Debug)]
pub enum PdfError {
    #[error("Failed to open PDF: {0}")]
    Open(String),
    
    #[error("Failed to save PDF: {0}")]
    Save(String),
    
    #[error("Invalid page number: {0}")]
    InvalidPage(usize),
    
    #[error("Failed to parse .rm file: {0}")]
    ParseRm(String),
    
    #[error("PDF error: {0}")]
    Lopdf(#[from] lopdf::Error),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// PDF annotator that overlays reMarkable strokes onto PDF pages
pub struct PdfAnnotator {
    doc: Document,
    /// reMarkable display dimensions (used for coordinate scaling)
    rm_width: f32,
    rm_height: f32,
}

impl PdfAnnotator {
    /// Open a PDF file for annotation
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, PdfError> {
        let doc = Document::load(path)
            .map_err(|e| PdfError::Open(e.to_string()))?;
        
        Ok(Self {
            doc,
            rm_width: 1872.0,
            rm_height: 1404.0,
        })
    }
    
    /// Create a new empty PDF document
    pub fn new() -> Self {
        Self {
            doc: Document::new(),
            rm_width: 1872.0,
            rm_height: 1404.0,
        }
    }
    
    /// Get the number of pages
    pub fn page_count(&self) -> usize {
        self.doc.get_pages().len()
    }
    
    /// Get page dimensions (width, height) in PDF points
    pub fn page_size(&self, page_num: usize) -> Result<(f32, f32), PdfError> {
        let pages = self.doc.get_pages();
        let page_id = pages.get(&(page_num as u32 + 1))
            .ok_or(PdfError::InvalidPage(page_num))?;
        
        if let Ok(page) = self.doc.get_dictionary(*page_id) {
            if let Ok(media_box) = page.get(b"MediaBox") {
                if let Ok(array) = media_box.as_array() {
                    if array.len() >= 4 {
                        let width = array[2].as_float().unwrap_or(612.0);
                        let height = array[3].as_float().unwrap_or(792.0);
                        return Ok((width, height));
                    }
                }
            }
        }
        
        // Default to US Letter size
        Ok((612.0, 792.0))
    }
    
    /// Add strokes to a specific page
    pub fn add_strokes(&mut self, page_num: usize, strokes: &[Stroke]) -> Result<(), PdfError> {
        if strokes.is_empty() {
            return Ok(());
        }
        
        let (pdf_width, pdf_height) = self.page_size(page_num)?;
        
        // Scale factors from reMarkable coordinates to PDF points
        let scale_x = pdf_width / self.rm_width;
        let scale_y = pdf_height / self.rm_height;
        
        // Build PDF content stream for strokes
        let mut operations = Vec::new();
        
        // Save graphics state
        operations.push(Operation::new("q", vec![]));
        
        // Set stroke color (black) and line properties
        operations.push(Operation::new("0", vec![]));
        operations.push(Operation::new("g", vec![])); // grayscale
        operations.push(Operation::new("1", vec![]));
        operations.push(Operation::new("J", vec![])); // round cap
        operations.push(Operation::new("1", vec![]));
        operations.push(Operation::new("j", vec![])); // round join
        
        for stroke in strokes {
            if stroke.points.is_empty() {
                continue;
            }
            
            // Set line width based on stroke
            let line_width = stroke.base_width * scale_x;
            operations.push(Operation::new("w", vec![line_width.into()]));
            
            // Set stroke color based on pen color
            let gray = match stroke.color {
                remarkable_core::Color::Black => 0.0,
                remarkable_core::Color::Gray => 0.5,
                remarkable_core::Color::White => 1.0,
                _ => 0.0,
            };
            operations.push(Operation::new("G", vec![gray.into()]));
            
            // Move to first point
            let first = &stroke.points[0];
            let x = first.x * scale_x;
            let y = pdf_height - (first.y * scale_y); // Flip Y axis
            operations.push(Operation::new("m", vec![x.into(), y.into()]));
            
            // Draw lines to remaining points
            for point in &stroke.points[1..] {
                let x = point.x * scale_x;
                let y = pdf_height - (point.y * scale_y);
                operations.push(Operation::new("l", vec![x.into(), y.into()]));
            }
            
            // Stroke the path
            operations.push(Operation::new("S", vec![]));
        }
        
        // Restore graphics state
        operations.push(Operation::new("Q", vec![]));
        
        // Create content stream
        let content = Content { operations };
        let stream_bytes = content.encode()?;
        
        // Add to page
        let pages = self.doc.get_pages();
        let page_id = pages.get(&(page_num as u32 + 1))
            .ok_or(PdfError::InvalidPage(page_num))?;
        
        // Create stream object
        let mut stream_dict = Dictionary::new();
        stream_dict.set("Length", stream_bytes.len() as i32);
        let stream = Stream::new(stream_dict, stream_bytes);
        let stream_id = self.doc.add_object(stream);
        
        // Append to page contents
        if let Ok(page) = self.doc.get_dictionary(*page_id).cloned() {
            if let Ok(contents) = page.get(b"Contents") {
                match contents {
                    Object::Array(arr) => {
                        let mut new_arr = arr.clone();
                        new_arr.push(Object::Reference(stream_id));
                        self.doc.get_dictionary_mut(*page_id)?
                            .set("Contents", new_arr);
                    }
                    Object::Reference(r) => {
                        self.doc.get_dictionary_mut(*page_id)?
                            .set("Contents", vec![Object::Reference(*r), Object::Reference(stream_id)]);
                    }
                    _ => {
                        self.doc.get_dictionary_mut(*page_id)?
                            .set("Contents", Object::Reference(stream_id));
                    }
                }
            } else {
                self.doc.get_dictionary_mut(*page_id)?
                    .set("Contents", Object::Reference(stream_id));
            }
        }
        
        Ok(())
    }
    
    /// Add strokes from an .rm file to a page
    pub fn add_rm_file<P: AsRef<Path>>(&mut self, page_num: usize, rm_path: P) -> Result<(), PdfError> {
        let data = std::fs::read(rm_path)?;
        let strokes = parse_rm_file(&data)
            .map_err(|e| PdfError::ParseRm(e.to_string()))?;
        self.add_strokes(page_num, &strokes)
    }
    
    /// Save the annotated PDF
    pub fn save<P: AsRef<Path>>(&mut self, path: P) -> Result<(), PdfError> {
        self.doc.save(path)
            .map_err(|e| PdfError::Save(e.to_string()))?;
        Ok(())
    }
}

impl Default for PdfAnnotator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use remarkable_core::{Point, PenType, Color};
    
    #[test]
    fn test_new_pdf() {
        let annotator = PdfAnnotator::new();
        assert_eq!(annotator.rm_width, 1872.0);
        assert_eq!(annotator.rm_height, 1404.0);
    }
    
    #[test]
    fn test_empty_strokes() {
        let mut annotator = PdfAnnotator::new();
        let result = annotator.add_strokes(0, &[]);
        assert!(result.is_ok());
    }
}
