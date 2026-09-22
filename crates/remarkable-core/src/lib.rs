//! Core types and traits for reMarkable documents
//!
//! This crate provides the fundamental data structures and trait-based
//! abstractions used across the remarkable-* crate ecosystem.
//!
//! # Architecture
//!
//! The library is organized into three layers:
//!
//! 1. **Types** (`types` module): Core data structures like `Stroke`, `Layer`, `Page`
//! 2. **Traits** (`traits` module): Abstract interfaces for format parsing, rendering, sync, etc.
//! 3. **Domain** (`crdt`, `template`, etc.): Domain-specific functionality
//!
//! # Example
//!
//! ```ignore
//! use remarkable_core::{Stroke, Point, PenType, Color};
//! use remarkable_core::traits::{Pen, StrokeColor, PenBehavior, ColorValue};
//!
//! // Create a stroke using core types
//! let mut stroke = Stroke::new(PenType::Fineliner1, Color::Black, 2.0);
//! stroke.push(Point::new(0.0, 0.0));
//! stroke.push(Point::new(100.0, 100.0));
//!
//! // Use trait-based pen system for rendering info
//! let pen = Pen::from_id(stroke.pen.to_u32());
//! println!("Pen: {} supports pressure: {}", pen.name(), pen.supports_pressure());
//! ```

mod types;

pub mod crdt;
pub mod template;
pub mod pdf_annotation;
pub mod traits;

// Export all types from the unified types module
pub use types::*;
pub use template::*;
pub use pdf_annotation::*;

/// Common result type
pub type Result<T> = std::result::Result<T, Error>;

/// Common error type
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Parse error: {0}")]
    Parse(String),
    
    #[error("Not found: {0}")]
    NotFound(String),
    
    #[error("Format error: {0}")]
    Format(#[from] traits::format::FormatError),
    
    #[error("Export error: {0}")]
    Export(#[from] traits::export::ExportError),
    
    #[error("Render error: {0}")]
    Render(#[from] traits::render::RenderError),
    
    #[error("Device error: {0}")]
    Device(#[from] traits::device::DeviceError),
    
    #[error("Auth error: {0}")]
    Auth(#[from] traits::auth::AuthError),
}
