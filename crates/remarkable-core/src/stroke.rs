//! Stroke and point types for handwriting data

use serde::{Deserialize, Serialize};

/// Pen tool types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u32)]
pub enum PenType {
    // Basic tools
    Brush = 0,
    Pencil = 1,
    Ballpoint = 2,
    Marker = 3,
    Fineliner = 4,
    Highlighter = 5,
    Eraser = 6,
    SharpPencil = 7,
    EraseArea = 8,
    
    // Version 2 tools
    CalligraphyPen = 13,
    Ballpointv2 = 15,
    Finelinerv2 = 17,
    Markerv2 = 16,
    Pencilv2 = 14,
    Highlighterv2 = 18,
    
    Unknown(u32),
}

impl PenType {
    pub fn from_u32(v: u32) -> Self {
        match v {
            0 => Self::Brush,
            1 => Self::Pencil,
            2 => Self::Ballpoint,
            3 => Self::Marker,
            4 => Self::Fineliner,
            5 => Self::Highlighter,
            6 => Self::Eraser,
            7 => Self::SharpPencil,
            8 => Self::EraseArea,
            13 => Self::CalligraphyPen,
            14 => Self::Pencilv2,
            15 => Self::Ballpointv2,
            16 => Self::Markerv2,
            17 => Self::Finelinerv2,
            18 => Self::Highlighterv2,
            _ => Self::Unknown(v),
        }
    }
}

/// Stroke colors
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u32)]
pub enum Color {
    Black = 0,
    Gray = 1,
    White = 2,
    Yellow = 3,
    Green = 4,
    Pink = 5,
    Blue = 6,
    Red = 7,
    GrayOverlap = 8,
    Unknown(u32),
}

impl Color {
    pub fn from_u32(v: u32) -> Self {
        match v {
            0 => Self::Black,
            1 => Self::Gray,
            2 => Self::White,
            3 => Self::Yellow,
            4 => Self::Green,
            5 => Self::Pink,
            6 => Self::Blue,
            7 => Self::Red,
            8 => Self::GrayOverlap,
            _ => Self::Unknown(v),
        }
    }
    
    pub fn to_rgb(&self) -> (u8, u8, u8) {
        match self {
            Self::Black => (0, 0, 0),
            Self::Gray => (128, 128, 128),
            Self::White => (255, 255, 255),
            Self::Yellow => (255, 255, 0),
            Self::Green => (0, 255, 0),
            Self::Pink => (255, 192, 203),
            Self::Blue => (0, 0, 255),
            Self::Red => (255, 0, 0),
            Self::GrayOverlap => (100, 100, 100),
            Self::Unknown(_) => (0, 0, 0),
        }
    }
}

/// A single point in a stroke
#[derive(Debug, Clone, Copy)]
pub struct Point {
    /// X coordinate (0-1404 typical, can extend beyond)
    pub x: f32,
    /// Y coordinate (0-1872 typical, scrollable beyond)
    pub y: f32,
    /// Drawing speed
    pub speed: u16,
    /// Stroke width
    pub width: u16,
    /// Pen direction (0-255 maps to 0-2π)
    pub direction: u8,
    /// Pen pressure (0-255)
    pub pressure: u8,
}

impl Point {
    /// Point data size in bytes (version 6 format)
    pub const SIZE_V6: usize = 14;
    
    /// Point data size in bytes (version 5 and earlier)
    pub const SIZE_V5: usize = 24;
}

/// A stroke (single pen movement)
#[derive(Debug, Clone)]
pub struct Stroke {
    /// Pen type
    pub pen: PenType,
    /// Stroke color
    pub color: Color,
    /// Base width
    pub base_width: f32,
    /// Points in the stroke
    pub points: Vec<Point>,
}

impl Stroke {
    /// Create a new empty stroke
    pub fn new(pen: PenType, color: Color, base_width: f32) -> Self {
        Self {
            pen,
            color,
            base_width,
            points: vec![],
        }
    }
    
    /// Convert stroke to SVG path
    pub fn to_svg_path(&self) -> String {
        if self.points.is_empty() {
            return String::new();
        }
        
        let mut path = format!("M {} {}", self.points[0].x, self.points[0].y);
        for point in &self.points[1..] {
            path.push_str(&format!(" L {} {}", point.x, point.y));
        }
        path
    }
}
