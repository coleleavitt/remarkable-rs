//! Core traits for the reMarkable library ecosystem
//!
//! This module defines the trait-based abstractions that enable:
//! - Format-agnostic document parsing and serialization
//! - Extensible pen and color systems
//! - Pluggable export backends
//! - Device-agnostic connection handling
//! - Flexible authentication providers
//! - Version-independent sync protocols
//!
//! # Design Principles
//!
//! 1. **Object Safety**: Traits are object-safe where possible (`dyn Trait`)
//! 2. **Associated Types**: Used for flexibility in implementations
//! 3. **Async Support**: Native async traits (Rust 1.75+)
//! 4. **Error Granularity**: Per-module error types for precise handling

pub mod format;
pub mod pen;
pub mod color;
pub mod crdt;
pub mod export;
pub mod device;
pub mod auth;
pub mod sync;
pub mod event;
pub mod render;
pub mod waveform;
pub mod template;

// Re-export primary traits
pub use format::{DocumentFormat, FormatVersion, ParsedDocument};
pub use pen::{PenBehavior, PenId, RenderParams, Pen, BallpointVariant, PencilVariant, EraserVariant, HighlighterVariant};
pub use color::{ColorValue, BlendMode, StrokeColor, HighlightColor};
pub use crdt::{CrdtOp, OpKind, LamportTimestamp, AuthorUuid};
pub use export::{Exporter, ExportFormat, SvgOptions, PngOptions, PdfOptions};
pub use device::{DeviceConnection, FileTransfer, DeviceInfo, FileEntry, ConnectionState};
pub use auth::{AuthProvider, TokenStore, TokenPair, AccessToken, RefreshToken};
pub use sync::{SyncProvider, SyncRoot, SyncDoc, Conflict, ConflictChoice, ProtocolVersion};
pub use event::{EventSource, EventStream, SyncEvent, ChangeKind};
pub use render::{StrokeRenderer, Canvas, RenderContext};
pub use waveform::{WaveformData, DisplayMode, TempRange, PanelInfo};
pub use template::{TemplateDefinition, TemplateCategory, TemplateRenderer};
