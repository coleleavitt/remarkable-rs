//! Pen type system with trait-based extensibility
//!
//! reMarkable supports 18+ pen types, each with unique rendering
//! characteristics. This module provides:
//!
//! - Type-safe pen identification
//! - Rendering parameter extraction
//! - Pressure/tilt capability queries
//! - Variant-aware pen categorization

use std::fmt;

/// Unique pen identifier (raw u32 from format)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PenId(pub u32);

impl PenId {
    pub const PAINTBRUSH_V1: Self = Self(0);
    pub const PENCIL_V1: Self = Self(1);
    pub const BALLPOINT_V1: Self = Self(2);
    pub const MARKER_V1: Self = Self(3);
    pub const FINELINER_V1: Self = Self(4);
    pub const HIGHLIGHTER_V1: Self = Self(5);
    pub const ERASER: Self = Self(6);
    pub const MECHANICAL_PENCIL_V1: Self = Self(7);
    pub const ERASER_SELECTION: Self = Self(8);
    pub const PAINTBRUSH_V2: Self = Self(12);
    pub const MECHANICAL_PENCIL_V2: Self = Self(13);
    pub const PENCIL_V2: Self = Self(14);
    pub const BALLPOINT_V2: Self = Self(15);
    pub const MARKER_V2: Self = Self(16);
    pub const FINELINER_V2: Self = Self(17);
    pub const HIGHLIGHTER_V2: Self = Self(18);
    pub const CALLIGRAPHY: Self = Self(21);
    pub const SHADER: Self = Self(23);
}

impl From<u32> for PenId {
    fn from(v: u32) -> Self {
        Self(v)
    }
}

impl From<PenId> for u32 {
    fn from(id: PenId) -> Self {
        id.0
    }
}

/// Rendering parameters for a pen stroke
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderParams {
    /// Base stroke width multiplier
    pub base_width: f32,
    /// Minimum stroke width
    pub min_width: f32,
    /// Maximum stroke width
    pub max_width: f32,
    /// Pressure sensitivity (0.0 = none, 1.0 = full)
    pub pressure_sensitivity: f32,
    /// Tilt sensitivity (0.0 = none, 1.0 = full)
    pub tilt_sensitivity: f32,
    /// Opacity (0.0 = transparent, 1.0 = opaque)
    pub opacity: f32,
    /// Whether to use texture
    pub textured: bool,
}

impl Default for RenderParams {
    fn default() -> Self {
        Self {
            base_width: 2.0,
            min_width: 0.5,
            max_width: 5.0,
            pressure_sensitivity: 0.8,
            tilt_sensitivity: 0.0,
            opacity: 1.0,
            textured: false,
        }
    }
}

impl RenderParams {
    /// Create for highlighter (semi-transparent, wide)
    pub fn highlighter() -> Self {
        Self {
            base_width: 15.0,
            min_width: 10.0,
            max_width: 20.0,
            pressure_sensitivity: 0.3,
            tilt_sensitivity: 0.0,
            opacity: 0.4,
            textured: false,
        }
    }
    
    /// Create for pencil (textured, pressure-sensitive)
    pub fn pencil() -> Self {
        Self {
            base_width: 2.0,
            min_width: 0.3,
            max_width: 4.0,
            pressure_sensitivity: 0.9,
            tilt_sensitivity: 0.5,
            opacity: 0.85,
            textured: true,
        }
    }
    
    /// Create for fineliner (consistent width)
    pub fn fineliner() -> Self {
        Self {
            base_width: 1.5,
            min_width: 1.5,
            max_width: 1.5,
            pressure_sensitivity: 0.0,
            tilt_sensitivity: 0.0,
            opacity: 1.0,
            textured: false,
        }
    }
    
    /// Create for calligraphy (tilt-sensitive)
    pub fn calligraphy() -> Self {
        Self {
            base_width: 3.0,
            min_width: 0.5,
            max_width: 8.0,
            pressure_sensitivity: 0.7,
            tilt_sensitivity: 1.0,
            opacity: 1.0,
            textured: false,
        }
    }
}

/// Core pen behavior trait
///
/// Implementations define rendering characteristics for each pen type.
/// This trait is object-safe for dynamic dispatch.
pub trait PenBehavior: Send + Sync {
    /// Get the pen identifier
    fn id(&self) -> PenId;
    
    /// Get the display name
    fn name(&self) -> &'static str;
    
    /// Get rendering parameters
    fn render_params(&self) -> RenderParams;
    
    /// Whether this pen responds to pressure
    fn supports_pressure(&self) -> bool {
        self.render_params().pressure_sensitivity > 0.0
    }
    
    /// Whether this pen responds to tilt
    fn supports_tilt(&self) -> bool {
        self.render_params().tilt_sensitivity > 0.0
    }
    
    /// Whether this is an eraser
    fn is_eraser(&self) -> bool {
        false
    }
    
    /// Whether this is a highlighter
    fn is_highlighter(&self) -> bool {
        false
    }
}

/// Ballpoint pen variants
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BallpointVariant {
    /// Original ballpoint (id=2)
    V1,
    /// Updated ballpoint with better pressure (id=15)
    V2,
}

impl BallpointVariant {
    pub fn id(&self) -> PenId {
        match self {
            Self::V1 => PenId::BALLPOINT_V1,
            Self::V2 => PenId::BALLPOINT_V2,
        }
    }
}

/// Pencil variants
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PencilVariant {
    /// Original pencil (id=1)
    V1,
    /// Updated pencil (id=14)
    V2,
    /// Mechanical pencil v1 (id=7)
    MechanicalV1,
    /// Mechanical pencil v2 (id=13)
    MechanicalV2,
}

impl PencilVariant {
    pub fn id(&self) -> PenId {
        match self {
            Self::V1 => PenId::PENCIL_V1,
            Self::V2 => PenId::PENCIL_V2,
            Self::MechanicalV1 => PenId::MECHANICAL_PENCIL_V1,
            Self::MechanicalV2 => PenId::MECHANICAL_PENCIL_V2,
        }
    }
}

/// Eraser variants
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EraserVariant {
    /// Point eraser (id=6)
    Normal,
    /// Selection/area eraser (id=8)
    Selection,
}

impl EraserVariant {
    pub fn id(&self) -> PenId {
        match self {
            Self::Normal => PenId::ERASER,
            Self::Selection => PenId::ERASER_SELECTION,
        }
    }
}

/// Highlighter variants
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HighlighterVariant {
    /// Original highlighter (id=5)
    V1,
    /// Updated highlighter (id=18)
    V2,
}

impl HighlighterVariant {
    pub fn id(&self) -> PenId {
        match self {
            Self::V1 => PenId::HIGHLIGHTER_V1,
            Self::V2 => PenId::HIGHLIGHTER_V2,
        }
    }
}

/// Marker variants
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MarkerVariant {
    /// Original marker (id=3)
    V1,
    /// Updated marker (id=16)
    V2,
}

/// Fineliner variants
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FinelinerVariant {
    /// Original fineliner (id=4)
    V1,
    /// Updated fineliner (id=17)
    V2,
}

/// Paintbrush variants
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaintbrushVariant {
    /// Original paintbrush (id=0)
    V1,
    /// Updated paintbrush (id=12)
    V2,
}

/// Unified pen type enum with variants
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pen {
    Ballpoint(BallpointVariant),
    Pencil(PencilVariant),
    Fineliner(FinelinerVariant),
    Marker(MarkerVariant),
    Paintbrush(PaintbrushVariant),
    Highlighter(HighlighterVariant),
    Eraser(EraserVariant),
    Calligraphy,
    Shader,
    Unknown(u32),
}

impl Pen {
    /// Create from raw pen ID
    pub fn from_id(id: u32) -> Self {
        match id {
            0 => Self::Paintbrush(PaintbrushVariant::V1),
            1 => Self::Pencil(PencilVariant::V1),
            2 => Self::Ballpoint(BallpointVariant::V1),
            3 => Self::Marker(MarkerVariant::V1),
            4 => Self::Fineliner(FinelinerVariant::V1),
            5 => Self::Highlighter(HighlighterVariant::V1),
            6 => Self::Eraser(EraserVariant::Normal),
            7 => Self::Pencil(PencilVariant::MechanicalV1),
            8 => Self::Eraser(EraserVariant::Selection),
            12 => Self::Paintbrush(PaintbrushVariant::V2),
            13 => Self::Pencil(PencilVariant::MechanicalV2),
            14 => Self::Pencil(PencilVariant::V2),
            15 => Self::Ballpoint(BallpointVariant::V2),
            16 => Self::Marker(MarkerVariant::V2),
            17 => Self::Fineliner(FinelinerVariant::V2),
            18 => Self::Highlighter(HighlighterVariant::V2),
            21 => Self::Calligraphy,
            23 => Self::Shader,
            _ => Self::Unknown(id),
        }
    }
    
    /// Get the raw pen ID
    pub fn id(&self) -> PenId {
        match self {
            Self::Paintbrush(v) => match v {
                PaintbrushVariant::V1 => PenId::PAINTBRUSH_V1,
                PaintbrushVariant::V2 => PenId::PAINTBRUSH_V2,
            },
            Self::Pencil(v) => v.id(),
            Self::Ballpoint(v) => v.id(),
            Self::Marker(v) => match v {
                MarkerVariant::V1 => PenId::MARKER_V1,
                MarkerVariant::V2 => PenId::MARKER_V2,
            },
            Self::Fineliner(v) => match v {
                FinelinerVariant::V1 => PenId::FINELINER_V1,
                FinelinerVariant::V2 => PenId::FINELINER_V2,
            },
            Self::Highlighter(v) => v.id(),
            Self::Eraser(v) => v.id(),
            Self::Calligraphy => PenId::CALLIGRAPHY,
            Self::Shader => PenId::SHADER,
            Self::Unknown(id) => PenId(*id),
        }
    }
}

impl PenBehavior for Pen {
    fn id(&self) -> PenId {
        self.id()
    }
    
    fn name(&self) -> &'static str {
        match self {
            Self::Ballpoint(BallpointVariant::V1) => "Ballpoint v1",
            Self::Ballpoint(BallpointVariant::V2) => "Ballpoint v2",
            Self::Pencil(PencilVariant::V1) => "Pencil v1",
            Self::Pencil(PencilVariant::V2) => "Pencil v2",
            Self::Pencil(PencilVariant::MechanicalV1) => "Mechanical Pencil v1",
            Self::Pencil(PencilVariant::MechanicalV2) => "Mechanical Pencil v2",
            Self::Fineliner(FinelinerVariant::V1) => "Fineliner v1",
            Self::Fineliner(FinelinerVariant::V2) => "Fineliner v2",
            Self::Marker(MarkerVariant::V1) => "Marker v1",
            Self::Marker(MarkerVariant::V2) => "Marker v2",
            Self::Paintbrush(PaintbrushVariant::V1) => "Paintbrush v1",
            Self::Paintbrush(PaintbrushVariant::V2) => "Paintbrush v2",
            Self::Highlighter(HighlighterVariant::V1) => "Highlighter v1",
            Self::Highlighter(HighlighterVariant::V2) => "Highlighter v2",
            Self::Eraser(EraserVariant::Normal) => "Eraser",
            Self::Eraser(EraserVariant::Selection) => "Selection Eraser",
            Self::Calligraphy => "Calligraphy",
            Self::Shader => "Shader",
            Self::Unknown(_) => "Unknown",
        }
    }
    
    fn render_params(&self) -> RenderParams {
        match self {
            Self::Ballpoint(_) => RenderParams {
                base_width: 2.0,
                min_width: 1.0,
                max_width: 3.0,
                pressure_sensitivity: 0.7,
                ..Default::default()
            },
            Self::Pencil(_) => RenderParams::pencil(),
            Self::Fineliner(_) => RenderParams::fineliner(),
            Self::Marker(_) => RenderParams {
                base_width: 3.5,
                min_width: 2.0,
                max_width: 5.0,
                pressure_sensitivity: 0.5,
                ..Default::default()
            },
            Self::Paintbrush(_) => RenderParams {
                base_width: 4.0,
                min_width: 1.0,
                max_width: 10.0,
                pressure_sensitivity: 0.95,
                textured: true,
                ..Default::default()
            },
            Self::Highlighter(_) => RenderParams::highlighter(),
            Self::Eraser(_) => RenderParams {
                base_width: 5.0,
                min_width: 3.0,
                max_width: 20.0,
                pressure_sensitivity: 0.5,
                opacity: 0.0, // Erases, doesn't draw
                ..Default::default()
            },
            Self::Calligraphy => RenderParams::calligraphy(),
            Self::Shader => RenderParams {
                base_width: 8.0,
                min_width: 2.0,
                max_width: 15.0,
                pressure_sensitivity: 0.6,
                opacity: 0.3,
                ..Default::default()
            },
            Self::Unknown(_) => RenderParams::default(),
        }
    }
    
    fn is_eraser(&self) -> bool {
        matches!(self, Self::Eraser(_))
    }
    
    fn is_highlighter(&self) -> bool {
        matches!(self, Self::Highlighter(_))
    }
}

impl fmt::Display for Pen {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

impl Default for Pen {
    fn default() -> Self {
        Self::Fineliner(FinelinerVariant::V2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_pen_roundtrip() {
        for id in [0, 1, 2, 3, 4, 5, 6, 7, 8, 12, 13, 14, 15, 16, 17, 18, 21, 23] {
            let pen = Pen::from_id(id);
            assert_eq!(pen.id().0, id);
        }
    }
    
    #[test]
    fn test_pen_capabilities() {
        let highlighter = Pen::Highlighter(HighlighterVariant::V2);
        assert!(highlighter.is_highlighter());
        assert!(!highlighter.is_eraser());
        
        let eraser = Pen::Eraser(EraserVariant::Normal);
        assert!(eraser.is_eraser());
        assert!(!eraser.is_highlighter());
        
        let fineliner = Pen::Fineliner(FinelinerVariant::V1);
        assert!(!fineliner.supports_pressure());
        
        let pencil = Pen::Pencil(PencilVariant::V2);
        assert!(pencil.supports_pressure());
    }
}
