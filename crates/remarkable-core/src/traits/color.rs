//! Color system with trait-based rendering properties
//!
//! reMarkable supports various colors across device generations:
//! - RM1/RM2: Grayscale only (black, gray, white)
//! - Paper Pro: Full color including highlights
//!
//! This module provides type-safe color handling with blend modes.

use std::fmt;

/// RGBA color value
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const BLACK: Self = Self { r: 0, g: 0, b: 0, a: 255 };
    pub const WHITE: Self = Self { r: 255, g: 255, b: 255, a: 255 };
    pub const GRAY: Self = Self { r: 128, g: 128, b: 128, a: 255 };
    pub const TRANSPARENT: Self = Self { r: 0, g: 0, b: 0, a: 0 };
    
    /// Create from RGB with full opacity
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }
    
    /// Create from RGBA
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
    
    /// Convert to hex string (#RRGGBB or #RRGGBBAA)
    pub fn to_hex(&self) -> String {
        if self.a == 255 {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a)
        }
    }
    
    /// Convert to CSS rgba() string
    pub fn to_css_rgba(&self) -> String {
        format!(
            "rgba({}, {}, {}, {:.3})",
            self.r, self.g, self.b, self.a as f32 / 255.0
        )
    }
    
    /// Parse from hex string
    pub fn from_hex(s: &str) -> Option<Self> {
        let s = s.strip_prefix('#').unwrap_or(s);
        match s.len() {
            6 => {
                let r = u8::from_str_radix(&s[0..2], 16).ok()?;
                let g = u8::from_str_radix(&s[2..4], 16).ok()?;
                let b = u8::from_str_radix(&s[4..6], 16).ok()?;
                Some(Self::rgb(r, g, b))
            }
            8 => {
                let r = u8::from_str_radix(&s[0..2], 16).ok()?;
                let g = u8::from_str_radix(&s[2..4], 16).ok()?;
                let b = u8::from_str_radix(&s[4..6], 16).ok()?;
                let a = u8::from_str_radix(&s[6..8], 16).ok()?;
                Some(Self::rgba(r, g, b, a))
            }
            _ => None,
        }
    }
    
    /// Convert to array
    pub const fn to_array(&self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }
    
    /// Create with modified alpha
    pub const fn with_alpha(&self, a: u8) -> Self {
        Self { a, ..*self }
    }
}

impl Default for Rgba {
    fn default() -> Self {
        Self::BLACK
    }
}

impl fmt::Display for Rgba {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

/// Blend mode for compositing strokes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BlendMode {
    /// Normal alpha blending (default)
    #[default]
    Normal,
    /// Multiply (darken, used for overlapping strokes)
    Multiply,
    /// Screen (lighten)
    Screen,
    /// Overlay
    Overlay,
    /// Color burn (for highlighter overlap)
    ColorBurn,
}

impl BlendMode {
    /// Get CSS mix-blend-mode value
    pub fn css_value(&self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Multiply => "multiply",
            Self::Screen => "screen",
            Self::Overlay => "overlay",
            Self::ColorBurn => "color-burn",
        }
    }
}

/// Core color value trait
///
/// Implementations provide RGBA values and rendering properties.
pub trait ColorValue: Send + Sync {
    /// Get the RGBA color value
    fn rgba(&self) -> Rgba;
    
    /// Get the blend mode for this color
    fn blend_mode(&self) -> BlendMode {
        BlendMode::Normal
    }
    
    /// Whether this is a highlight color (semi-transparent, special blend)
    fn is_highlight(&self) -> bool {
        false
    }
    
    /// Whether this is for erasing (clears rather than draws)
    fn is_eraser(&self) -> bool {
        false
    }
    
    /// Get CSS color string
    fn to_css(&self) -> String {
        self.rgba().to_css_rgba()
    }
}

/// Standard stroke colors from format
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StrokeColor {
    /// Pure black (#000000)
    Black,
    /// Medium gray (#888888)
    Gray,
    /// Pure white (#ffffff)
    White,
    /// Yellow (Paper Pro)
    Yellow,
    /// Green (Paper Pro)
    Green,
    /// Pink (Paper Pro)
    Pink,
    /// Blue (Paper Pro)
    Blue,
    /// Red (Paper Pro)
    Red,
    /// Gray for overlap (#666666)
    GrayOverlap,
    /// Highlight (actual color stored separately)
    Highlight,
    /// Green variant 2
    Green2,
    /// Cyan
    Cyan,
    /// Magenta
    Magenta,
    /// Yellow variant 2
    Yellow2,
    /// Unknown color from format
    Unknown(u32),
}

impl StrokeColor {
    /// Create from raw color ID
    pub fn from_id(id: u32) -> Self {
        match id {
            0 => Self::Black,
            1 => Self::Gray,
            2 => Self::White,
            3 => Self::Yellow,
            4 => Self::Green,
            5 => Self::Pink,
            6 => Self::Blue,
            7 => Self::Red,
            8 => Self::GrayOverlap,
            9 => Self::Highlight,
            10 => Self::Green2,
            11 => Self::Cyan,
            12 => Self::Magenta,
            13 => Self::Yellow2,
            _ => Self::Unknown(id),
        }
    }
    
    /// Get raw color ID
    pub fn id(&self) -> u32 {
        match self {
            Self::Black => 0,
            Self::Gray => 1,
            Self::White => 2,
            Self::Yellow => 3,
            Self::Green => 4,
            Self::Pink => 5,
            Self::Blue => 6,
            Self::Red => 7,
            Self::GrayOverlap => 8,
            Self::Highlight => 9,
            Self::Green2 => 10,
            Self::Cyan => 11,
            Self::Magenta => 12,
            Self::Yellow2 => 13,
            Self::Unknown(id) => *id,
        }
    }
}

impl ColorValue for StrokeColor {
    fn rgba(&self) -> Rgba {
        match self {
            Self::Black => Rgba::BLACK,
            Self::Gray => Rgba::rgb(0x88, 0x88, 0x88),
            Self::White => Rgba::WHITE,
            Self::Yellow => Rgba::rgb(0xf0, 0xc0, 0x20),
            Self::Green => Rgba::rgb(0x00, 0xa0, 0x00),
            Self::Pink => Rgba::rgb(0xff, 0x80, 0xc0),
            Self::Blue => Rgba::rgb(0x00, 0x60, 0xff),
            Self::Red => Rgba::rgb(0xff, 0x00, 0x00),
            Self::GrayOverlap => Rgba::rgb(0x66, 0x66, 0x66),
            Self::Highlight => Rgba::rgba(0xff, 0xff, 0x00, 0x66),
            Self::Green2 => Rgba::rgb(0x00, 0xff, 0x00),
            Self::Cyan => Rgba::rgb(0x00, 0xff, 0xff),
            Self::Magenta => Rgba::rgb(0xff, 0x00, 0xff),
            Self::Yellow2 => Rgba::rgb(0xff, 0xff, 0x00),
            Self::Unknown(_) => Rgba::BLACK,
        }
    }
    
    fn is_highlight(&self) -> bool {
        matches!(self, Self::Highlight)
    }
}

impl Default for StrokeColor {
    fn default() -> Self {
        Self::Black
    }
}

impl fmt::Display for StrokeColor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Black => write!(f, "Black"),
            Self::Gray => write!(f, "Gray"),
            Self::White => write!(f, "White"),
            Self::Yellow => write!(f, "Yellow"),
            Self::Green => write!(f, "Green"),
            Self::Pink => write!(f, "Pink"),
            Self::Blue => write!(f, "Blue"),
            Self::Red => write!(f, "Red"),
            Self::GrayOverlap => write!(f, "Gray (overlap)"),
            Self::Highlight => write!(f, "Highlight"),
            Self::Green2 => write!(f, "Green 2"),
            Self::Cyan => write!(f, "Cyan"),
            Self::Magenta => write!(f, "Magenta"),
            Self::Yellow2 => write!(f, "Yellow 2"),
            Self::Unknown(id) => write!(f, "Unknown({id})"),
        }
    }
}

/// Highlight colors (semi-transparent for marking)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HighlightColor {
    Yellow,
    Green,
    Pink,
    Blue,
    Orange,
    Gray,
}

impl HighlightColor {
    /// Default highlight opacity (40%)
    pub const DEFAULT_OPACITY: u8 = 102; // 0.4 * 255
}

impl ColorValue for HighlightColor {
    fn rgba(&self) -> Rgba {
        let (r, g, b) = match self {
            Self::Yellow => (0xff, 0xff, 0x00),
            Self::Green => (0x00, 0xff, 0x00),
            Self::Pink => (0xff, 0x80, 0xc0),
            Self::Blue => (0x00, 0x80, 0xff),
            Self::Orange => (0xff, 0x80, 0x00),
            Self::Gray => (0x80, 0x80, 0x80),
        };
        Rgba::rgba(r, g, b, Self::DEFAULT_OPACITY)
    }
    
    fn blend_mode(&self) -> BlendMode {
        BlendMode::Multiply
    }
    
    fn is_highlight(&self) -> bool {
        true
    }
}

impl Default for HighlightColor {
    fn default() -> Self {
        Self::Yellow
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_rgba_hex_roundtrip() {
        let color = Rgba::rgb(0xab, 0xcd, 0xef);
        assert_eq!(color.to_hex(), "#abcdef");
        assert_eq!(Rgba::from_hex("#abcdef"), Some(color));
        assert_eq!(Rgba::from_hex("abcdef"), Some(color));
    }
    
    #[test]
    fn test_stroke_color_ids() {
        for id in 0..14 {
            let color = StrokeColor::from_id(id);
            assert_eq!(color.id(), id);
        }
    }
    
    #[test]
    fn test_highlight_properties() {
        let highlight = HighlightColor::Yellow;
        assert!(highlight.is_highlight());
        assert_eq!(highlight.blend_mode(), BlendMode::Multiply);
        assert!(highlight.rgba().a < 255);
    }
}
