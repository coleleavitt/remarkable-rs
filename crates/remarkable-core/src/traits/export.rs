//! Export format traits for document conversion
//!
//! This module provides a unified interface for exporting documents
//! to various formats: SVG, PNG, PDF, JSON, etc.
//!
//! # Design
//!
//! The `Exporter` trait uses associated types for:
//! - `Output`: The result type (String, Vec<u8>, etc.)
//! - `Options`: Configuration for the export

use crate::{Document, Layer, Page, Stroke};

/// Supported export formats
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExportFormat {
    /// Scalable Vector Graphics
    Svg,
    /// Portable Network Graphics
    Png,
    /// Portable Document Format
    Pdf,
    /// JSON representation
    Json,
    /// Raw .rm format (re-serialization)
    Rm,
}

impl ExportFormat {
    /// Get file extension for this format
    pub const fn extension(&self) -> &'static str {
        match self {
            Self::Svg => "svg",
            Self::Png => "png",
            Self::Pdf => "pdf",
            Self::Json => "json",
            Self::Rm => "rm",
        }
    }
    
    /// Get MIME type for this format
    pub const fn mime_type(&self) -> &'static str {
        match self {
            Self::Svg => "image/svg+xml",
            Self::Png => "image/png",
            Self::Pdf => "application/pdf",
            Self::Json => "application/json",
            Self::Rm => "application/octet-stream",
        }
    }
}

/// Error type for export operations
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("no strokes to export")]
    EmptyDocument,
    
    #[error("invalid dimensions: {0}")]
    InvalidDimensions(String),
    
    #[error("rendering failed: {0}")]
    RenderFailed(String),
    
    #[error("serialization failed: {0}")]
    SerializationFailed(String),
    
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// SVG export options
#[derive(Debug, Clone)]
pub struct SvgOptions {
    /// Output width in pixels
    pub width: u32,
    /// Output height in pixels
    pub height: u32,
    /// Background color (CSS format)
    pub background: String,
    /// Whether to include a background rectangle
    pub include_background: bool,
    /// Stroke linecap style
    pub linecap: String,
    /// Stroke linejoin style
    pub linejoin: String,
    /// Whether to optimize stroke paths
    pub optimize: bool,
    /// CSS class prefix for styling
    pub class_prefix: Option<String>,
}

impl Default for SvgOptions {
    fn default() -> Self {
        Self {
            width: 1404,
            height: 1872,
            background: "white".to_string(),
            include_background: true,
            linecap: "round".to_string(),
            linejoin: "round".to_string(),
            optimize: false,
            class_prefix: None,
        }
    }
}

impl SvgOptions {
    /// Create with reMarkable page dimensions
    pub fn remarkable_page() -> Self {
        Self::default()
    }
    
    /// Create with reMarkable landscape dimensions
    pub fn remarkable_landscape() -> Self {
        Self {
            width: 1872,
            height: 1404,
            ..Default::default()
        }
    }
    
    /// Set dimensions
    pub fn with_size(mut self, width: u32, height: u32) -> Self {
        self.width = width;
        self.height = height;
        self
    }
    
    /// Set background color
    pub fn with_background(mut self, color: impl Into<String>) -> Self {
        self.background = color.into();
        self
    }
    
    /// Disable background rectangle
    pub fn no_background(mut self) -> Self {
        self.include_background = false;
        self
    }
}

/// PNG export options
#[derive(Debug, Clone)]
pub struct PngOptions {
    /// Output width in pixels
    pub width: u32,
    /// Output height in pixels
    pub height: u32,
    /// Background color (RGBA)
    pub background: [u8; 4],
    /// Scaling factor (1.0 = native)
    pub scale: f32,
    /// Whether to include transparent background
    pub transparent: bool,
    /// Anti-aliasing enabled
    pub antialias: bool,
}

impl Default for PngOptions {
    fn default() -> Self {
        Self {
            width: 1404,
            height: 1872,
            background: [255, 255, 255, 255],
            scale: 1.0,
            transparent: false,
            antialias: true,
        }
    }
}

impl PngOptions {
    /// Create with scaling factor
    pub fn with_scale(mut self, scale: f32) -> Self {
        self.scale = scale;
        self.width = (self.width as f32 * scale) as u32;
        self.height = (self.height as f32 * scale) as u32;
        self
    }
    
    /// Enable transparent background
    pub fn transparent(mut self) -> Self {
        self.transparent = true;
        self.background[3] = 0;
        self
    }
}

/// PDF export options
#[derive(Debug, Clone)]
pub struct PdfOptions {
    /// Page width in points (72 points = 1 inch)
    pub page_width: f32,
    /// Page height in points
    pub page_height: f32,
    /// Document title
    pub title: Option<String>,
    /// Document author
    pub author: Option<String>,
    /// Whether to embed fonts
    pub embed_fonts: bool,
    /// Compression level (0-9)
    pub compression: u8,
}

impl Default for PdfOptions {
    fn default() -> Self {
        // A4 portrait (210mm × 297mm)
        Self {
            page_width: 595.0,
            page_height: 842.0,
            title: None,
            author: None,
            embed_fonts: true,
            compression: 6,
        }
    }
}

impl PdfOptions {
    /// Create with letter size (8.5" × 11")
    pub fn letter() -> Self {
        Self {
            page_width: 612.0,
            page_height: 792.0,
            ..Default::default()
        }
    }
    
    /// Create with reMarkable native aspect ratio
    pub fn remarkable() -> Self {
        // Scale to fit A4 width while preserving aspect ratio
        Self {
            page_width: 595.0,
            page_height: 792.0, // Maintains ~1:1.33 ratio
            ..Default::default()
        }
    }
    
    /// Set document title
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
}

/// Core exporter trait
///
/// Implementations convert documents to specific output formats.
///
/// # Associated Types
///
/// - `Output`: The result type (String for SVG, Vec<u8> for PNG)
/// - `Options`: Configuration type (default if not customized)
///
/// # Example
///
/// ```ignore
/// use remarkable_core::traits::{Exporter, SvgOptions};
///
/// let exporter = SvgExporter::new();
/// let svg = exporter.export(&document, SvgOptions::default())?;
/// ```
pub trait Exporter: Send + Sync {
    /// Output type produced by this exporter
    type Output;
    
    /// Configuration options for export
    type Options: Default;
    
    /// Get the export format
    fn format(&self) -> ExportFormat;
    
    /// Export a full document
    fn export(&self, doc: &Document, opts: Self::Options) -> Result<Self::Output, ExportError>;
    
    /// Export a single page
    fn export_page(&self, page: &Page, opts: Self::Options) -> Result<Self::Output, ExportError>;
    
    /// Export strokes directly
    fn export_strokes(&self, strokes: &[Stroke], opts: Self::Options) -> Result<Self::Output, ExportError>;
    
    /// Export a layer
    fn export_layer(&self, layer: &Layer, opts: Self::Options) -> Result<Self::Output, ExportError> {
        self.export_strokes(&layer.strokes, opts)
    }
}

/// Convenience trait for exporters that produce bytes
pub trait ByteExporter: Exporter<Output = Vec<u8>> {
    /// Export and write to file
    fn export_to_file(&self, doc: &Document, path: &std::path::Path, opts: Self::Options) -> Result<(), ExportError> {
        let data = self.export(doc, opts)?;
        std::fs::write(path, data)?;
        Ok(())
    }
}

// Implement ByteExporter for any Exporter producing Vec<u8>
impl<T> ByteExporter for T where T: Exporter<Output = Vec<u8>> {}

/// Convenience trait for exporters that produce strings
pub trait StringExporter: Exporter<Output = String> {
    /// Export and write to file
    fn export_to_file(&self, doc: &Document, path: &std::path::Path, opts: Self::Options) -> Result<(), ExportError> {
        let data = self.export(doc, opts)?;
        std::fs::write(path, data)?;
        Ok(())
    }
}

// Implement StringExporter for any Exporter producing String
impl<T> StringExporter for T where T: Exporter<Output = String> {}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_export_format_metadata() {
        assert_eq!(ExportFormat::Svg.extension(), "svg");
        assert_eq!(ExportFormat::Svg.mime_type(), "image/svg+xml");
        assert_eq!(ExportFormat::Png.extension(), "png");
        assert_eq!(ExportFormat::Pdf.extension(), "pdf");
    }
    
    #[test]
    fn test_svg_options_builder() {
        let opts = SvgOptions::default()
            .with_size(1000, 800)
            .with_background("black")
            .no_background();
        
        assert_eq!(opts.width, 1000);
        assert_eq!(opts.height, 800);
        assert_eq!(opts.background, "black");
        assert!(!opts.include_background);
    }
    
    #[test]
    fn test_png_options_scale() {
        let opts = PngOptions::default().with_scale(2.0);
        assert_eq!(opts.width, 2808);
        assert_eq!(opts.height, 3744);
        assert_eq!(opts.scale, 2.0);
    }
}
