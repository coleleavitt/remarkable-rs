//! Core types for reMarkable documents

use serde::{Deserialize, Serialize};

/// Point in a stroke with all attributes
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Point {
    /// X coordinate
    pub x: f32,
    /// Y coordinate  
    pub y: f32,
    /// Speed (encoded)
    pub speed: u16,
    /// Width (encoded)
    pub width: u16,
    /// Direction/tilt (0-255)
    pub direction: u8,
    /// Pressure (0-255)
    pub pressure: u8,
}

impl Point {
    /// Size in bytes for v6 format
    pub const SIZE_V6: usize = 14;
    
    /// Create a new point
    pub fn new(x: f32, y: f32) -> Self {
        Self {
            x,
            y,
            ..Default::default()
        }
    }
    
    /// Create with all attributes
    pub fn with_attrs(x: f32, y: f32, speed: u16, width: u16, direction: u8, pressure: u8) -> Self {
        Self { x, y, speed, width, direction, pressure }
    }
    
    /// Get normalized pressure (0.0 - 1.0)
    pub fn normalized_pressure(&self) -> f32 {
        self.pressure as f32 / 255.0
    }
    
    /// Get normalized direction in radians (0.0 - 2π)
    pub fn direction_radians(&self) -> f32 {
        (self.direction as f32 / 255.0) * std::f32::consts::TAU
    }
}

/// Pen types available on reMarkable
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u32)]
pub enum PenType {
    Paintbrush1 = 0,
    Pencil1 = 1,
    Ballpoint1 = 2,
    Marker1 = 3,
    Fineliner1 = 4,
    Highlighter1 = 5,
    Eraser = 6,
    MechanicalPencil1 = 7,
    EraserArea = 8,
    Paintbrush2 = 12,
    MechanicalPencil2 = 13,
    Pencil2 = 14,
    Ballpoint2 = 15,
    Marker2 = 16,
    Fineliner2 = 17,
    Highlighter2 = 18,
    Calligraphy = 21,
    Shader = 23,
    Unknown(u32),
}

impl PenType {
    pub fn from_u32(v: u32) -> Self {
        match v {
            0 => Self::Paintbrush1,
            1 => Self::Pencil1,
            2 => Self::Ballpoint1,
            3 => Self::Marker1,
            4 => Self::Fineliner1,
            5 => Self::Highlighter1,
            6 => Self::Eraser,
            7 => Self::MechanicalPencil1,
            8 => Self::EraserArea,
            12 => Self::Paintbrush2,
            13 => Self::MechanicalPencil2,
            14 => Self::Pencil2,
            15 => Self::Ballpoint2,
            16 => Self::Marker2,
            17 => Self::Fineliner2,
            18 => Self::Highlighter2,
            21 => Self::Calligraphy,
            23 => Self::Shader,
            _ => Self::Unknown(v),
        }
    }
    
    pub fn to_u32(&self) -> u32 {
        match self {
            Self::Paintbrush1 => 0,
            Self::Pencil1 => 1,
            Self::Ballpoint1 => 2,
            Self::Marker1 => 3,
            Self::Fineliner1 => 4,
            Self::Highlighter1 => 5,
            Self::Eraser => 6,
            Self::MechanicalPencil1 => 7,
            Self::EraserArea => 8,
            Self::Paintbrush2 => 12,
            Self::MechanicalPencil2 => 13,
            Self::Pencil2 => 14,
            Self::Ballpoint2 => 15,
            Self::Marker2 => 16,
            Self::Fineliner2 => 17,
            Self::Highlighter2 => 18,
            Self::Calligraphy => 21,
            Self::Shader => 23,
            Self::Unknown(v) => *v,
        }
    }
    
    /// Returns true if this pen is a highlighter
    pub fn is_highlighter(&self) -> bool {
        matches!(self, Self::Highlighter1 | Self::Highlighter2)
    }
    
    /// Returns true if this is an eraser tool
    pub fn is_eraser(&self) -> bool {
        matches!(self, Self::Eraser | Self::EraserArea)
    }
}

impl Default for PenType {
    fn default() -> Self {
        Self::Fineliner1
    }
}

/// Colors available on reMarkable
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
    /// Highlight color (actual RGBA stored separately)
    Highlight = 9,
    Green2 = 10,
    Cyan = 11,
    Magenta = 12,
    Yellow2 = 13,
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
            9 => Self::Highlight,
            10 => Self::Green2,
            11 => Self::Cyan,
            12 => Self::Magenta,
            13 => Self::Yellow2,
            _ => Self::Unknown(v),
        }
    }
    
    pub fn to_u32(&self) -> u32 {
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
            Self::Unknown(v) => *v,
        }
    }
    
    /// Convert to RGB hex color
    pub fn to_rgb(&self) -> &'static str {
        match self {
            Self::Black => "#000000",
            Self::Gray => "#888888",
            Self::White => "#ffffff",
            Self::Yellow => "#f0c020",
            Self::Green => "#00a000",
            Self::Pink => "#ff80c0",
            Self::Blue => "#0060ff",
            Self::Red => "#ff0000",
            Self::GrayOverlap => "#666666",
            Self::Highlight => "#ffff00",
            Self::Green2 => "#00ff00",
            Self::Cyan => "#00ffff",
            Self::Magenta => "#ff00ff",
            Self::Yellow2 => "#ffff00",
            Self::Unknown(_) => "#000000",
        }
    }
}

impl Default for Color {
    fn default() -> Self {
        Self::Black
    }
}

/// A stroke (line) on a page
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Stroke {
    /// Pen type used
    pub pen: PenType,
    /// Color of the stroke
    pub color: Color,
    /// Base width multiplier
    pub base_width: f32,
    /// Points in the stroke
    pub points: Vec<Point>,
}

impl Stroke {
    /// Create a new stroke
    pub fn new(pen: PenType, color: Color, base_width: f32) -> Self {
        Self {
            pen,
            color,
            base_width,
            points: Vec::new(),
        }
    }
    
    /// Add a point to the stroke
    pub fn push(&mut self, point: Point) {
        self.points.push(point);
    }
    
    /// Get bounding box (min_x, min_y, max_x, max_y)
    pub fn bounds(&self) -> Option<(f32, f32, f32, f32)> {
        if self.points.is_empty() {
            return None;
        }
        
        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_y = f32::MIN;
        
        for p in &self.points {
            min_x = min_x.min(p.x);
            min_y = min_y.min(p.y);
            max_x = max_x.max(p.x);
            max_y = max_y.max(p.y);
        }
        
        Some((min_x, min_y, max_x, max_y))
    }
    
    /// Convert to SVG path data
    pub fn to_svg_path(&self) -> String {
        if self.points.len() < 2 {
            return String::new();
        }
        
        let mut path = format!("M {} {}", self.points[0].x, self.points[0].y);
        for p in &self.points[1..] {
            path.push_str(&format!(" L {} {}", p.x, p.y));
        }
        path
    }
}

/// A layer containing strokes
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Layer {
    /// Layer name
    pub name: String,
    /// Whether the layer is visible
    pub visible: bool,
    /// Strokes in this layer
    pub strokes: Vec<Stroke>,
}

impl Layer {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            visible: true,
            strokes: Vec::new(),
        }
    }
}

/// A page in a document
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Page {
    /// Page ID (UUID)
    pub id: String,
    /// Layers on this page
    pub layers: Vec<Layer>,
}

impl Page {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            layers: Vec::new(),
        }
    }
    
    /// Get all strokes across all layers
    pub fn all_strokes(&self) -> impl Iterator<Item = &Stroke> {
        self.layers.iter().flat_map(|l| &l.strokes)
    }
}

/// A reMarkable document
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Document {
    /// Document ID (UUID)
    pub id: String,
    /// Document name
    pub name: String,
    /// Parent folder ID
    pub parent: String,
    /// Document type (DocumentType)
    pub doc_type: DocumentType,
    /// Pages in the document
    pub pages: Vec<Page>,
    /// Last modified timestamp
    pub modified: String,
}

/// Document type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DocumentType {
    #[default]
    Notebook,
    Pdf,
    Epub,
    Folder,
}

/// CRDT identifier used in v6 format
/// Composed of author ID (part1) and sequence number (part2)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CrdtId {
    pub part1: u64,
    pub part2: u64,
}

impl CrdtId {
    pub fn new(part1: u64, part2: u64) -> Self {
        Self { part1, part2 }
    }
    
    pub fn zero() -> Self {
        Self { part1: 0, part2: 0 }
    }
}

impl std::fmt::Display for CrdtId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CrdtId({}, {})", self.part1, self.part2)
    }
}


// Document metadata types

/// Document metadata from .metadata files
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentMetadata {
    pub created_time: String,
    pub last_modified: String,
    #[serde(default)]
    pub last_opened: Option<String>,
    #[serde(default)]
    pub last_opened_page: Option<i32>,
    pub parent: String,
    pub pinned: bool,
    #[serde(rename = "type")]
    pub doc_type: String,
    pub visible_name: String,
}

impl DocumentMetadata {
    pub fn is_folder(&self) -> bool {
        self.doc_type == "CollectionType"
    }
    
    pub fn is_document(&self) -> bool {
        self.doc_type == "DocumentType"
    }
}

/// CRDT timestamp (author_id:sequence)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrdtTimestamp {
    pub timestamp: String,
    pub value: serde_json::Value,
}

/// Page entry in .content file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentPage {
    pub id: String,
    pub idx: CrdtTimestamp,
    #[serde(default)]
    pub template: Option<CrdtTimestamp>,
    #[serde(rename = "scrollTime")]
    #[serde(default)]
    pub scroll_time: Option<CrdtTimestamp>,
    #[serde(rename = "verticalScroll")]
    #[serde(default)]
    pub vertical_scroll: Option<CrdtTimestamp>,
}

/// UUID pair for CRDT authorship
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UuidPair {
    pub first: String,
    pub second: i32,
}

/// CRDT pages container
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CPages {
    #[serde(rename = "lastOpened")]
    #[serde(default)]
    pub last_opened: Option<CrdtTimestamp>,
    #[serde(default)]
    pub original: Option<CrdtTimestamp>,
    pub pages: Vec<ContentPage>,
    #[serde(default)]
    pub uuids: Vec<UuidPair>,
}

/// Extra metadata for pen settings
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExtraMetadata {
    #[serde(rename = "LastPen")]
    #[serde(default)]
    pub last_pen: Option<String>,
    #[serde(rename = "LastTool")]
    #[serde(default)]
    pub last_tool: Option<String>,
    #[serde(rename = "LastBallpointv2Color")]
    #[serde(default)]
    pub last_ballpoint_color: Option<String>,
    #[serde(rename = "LastBallpointv2Size")]
    #[serde(default)]
    pub last_ballpoint_size: Option<String>,
    // Add more as needed
}

/// Document content from .content files
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentContent {
    #[serde(rename = "cPages")]
    #[serde(default)]
    pub c_pages: Option<CPages>,
    #[serde(default)]
    pub cover_page_number: Option<i32>,
    #[serde(default)]
    pub custom_zoom_center_x: Option<f32>,
    #[serde(default)]
    pub custom_zoom_center_y: Option<f32>,
    #[serde(default)]
    pub custom_zoom_orientation: Option<String>,
    #[serde(default)]
    pub custom_zoom_page_height: Option<i32>,
    #[serde(default)]
    pub custom_zoom_page_width: Option<i32>,
    #[serde(default)]
    pub custom_zoom_scale: Option<f32>,
    #[serde(default)]
    pub extra_metadata: Option<ExtraMetadata>,
    #[serde(default)]
    pub file_type: Option<String>,
    #[serde(default)]
    pub font_name: Option<String>,
    #[serde(default)]
    pub format_version: Option<i32>,
    #[serde(default)]
    pub line_height: Option<i32>,
    #[serde(default)]
    pub margins: Option<i32>,
    #[serde(default)]
    pub orientation: Option<String>,
    #[serde(default)]
    pub page_count: Option<i32>,
    #[serde(default)]
    pub size_in_bytes: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub text_alignment: Option<String>,
    #[serde(default)]
    pub text_scale: Option<f32>,
    #[serde(default)]
    pub zoom_mode: Option<String>,
}

impl DocumentContent {
    /// Get page UUIDs in order
    pub fn page_ids(&self) -> Vec<String> {
        self.c_pages
            .as_ref()
            .map(|cp| cp.pages.iter().map(|p| p.id.clone()).collect())
            .unwrap_or_default()
    }
    
    /// Get template for a specific page
    pub fn page_template(&self, page_id: &str) -> Option<String> {
        self.c_pages.as_ref().and_then(|cp| {
            cp.pages.iter()
                .find(|p| p.id == page_id)
                .and_then(|p| p.template.as_ref())
                .and_then(|t| t.value.as_str().map(String::from))
        })
    }
}
