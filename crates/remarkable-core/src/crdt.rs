//! CRDT (Conflict-free Replicated Data Type) primitives
//!
//! reMarkable uses Lamport timestamps for conflict-free merging:
//! - Format: "replica:counter" (e.g., "2:5")
//! - Higher timestamp wins (last-writer-wins)
//! - Same timestamp: higher replica ID wins
//!
//! The v6 format uses 14 CRDT operation types for document editing.

use serde::{Deserialize, Serialize};

/// Lamport timestamp for CRDT operations
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Timestamp {
    /// Device/replica identifier
    pub replica: u64,
    /// Monotonic logical clock
    pub counter: u64,
}

impl Timestamp {
    pub fn new(replica: u64, counter: u64) -> Self {
        Self { replica, counter }
    }
    
    /// Parse from string format "replica:counter"
    pub fn parse(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() != 2 {
            return None;
        }
        Some(Self {
            replica: parts[0].parse().ok()?,
            counter: parts[1].parse().ok()?,
        })
    }
    
    /// Compare timestamps for conflict resolution
    /// Returns true if self is newer than other
    pub fn is_newer_than(&self, other: &Self) -> bool {
        if self.counter != other.counter {
            self.counter > other.counter
        } else {
            self.replica > other.replica
        }
    }
    
    /// Increment the counter
    pub fn increment(&mut self) {
        self.counter += 1;
    }
}

impl std::fmt::Display for Timestamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.replica, self.counter)
    }
}

/// A value with CRDT timestamp
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrdtValue<T> {
    pub timestamp: String,
    pub value: T,
}

impl<T> CrdtValue<T> {
    pub fn new(timestamp: Timestamp, value: T) -> Self {
        Self {
            timestamp: timestamp.to_string(),
            value,
        }
    }
    
    pub fn parsed_timestamp(&self) -> Option<Timestamp> {
        Timestamp::parse(&self.timestamp)
    }
}

/// CRDT Operation types used in v6 .rm format
/// 
/// These operations are used for conflict-free document editing
/// and sync. Each operation carries an author ID for attribution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum CrdtOperation {
    /// Add a single item (stroke, text, etc.)
    AddItem = 0,
    /// Add multiple items at once
    AddItems = 1,
    /// Delete items by ID
    DeleteItems = 2,
    /// Delete and append items atomically
    DeleteAppendItems = 3,
    /// Insert items after a specific item
    InsertItemsAfter = 4,
    /// Swap positions of items
    SwapItems = 5,
    /// Create a new group/layer
    CreateGroup = 6,
    /// Delete a group/layer
    DeleteGroup = 7,
    /// Add a new layer
    AddLayer = 8,
    /// Delete a layer
    DeleteLayer = 9,
    /// Move layer to different position
    MoveLayer = 10,
    /// Merge layer down (combine with layer below)
    MergeLayerDown = 11,
    /// Set layer name
    SetLayerName = 12,
    /// Set layer visibility
    SetLayerVisible = 13,
}

impl CrdtOperation {
    /// Convert from raw u8 value
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::AddItem),
            1 => Some(Self::AddItems),
            2 => Some(Self::DeleteItems),
            3 => Some(Self::DeleteAppendItems),
            4 => Some(Self::InsertItemsAfter),
            5 => Some(Self::SwapItems),
            6 => Some(Self::CreateGroup),
            7 => Some(Self::DeleteGroup),
            8 => Some(Self::AddLayer),
            9 => Some(Self::DeleteLayer),
            10 => Some(Self::MoveLayer),
            11 => Some(Self::MergeLayerDown),
            12 => Some(Self::SetLayerName),
            13 => Some(Self::SetLayerVisible),
            _ => None,
        }
    }
    
    /// Check if this is a layer operation
    pub fn is_layer_operation(&self) -> bool {
        matches!(
            self,
            Self::AddLayer
                | Self::DeleteLayer
                | Self::MoveLayer
                | Self::MergeLayerDown
                | Self::SetLayerName
                | Self::SetLayerVisible
        )
    }
    
    /// Check if this is an item operation
    pub fn is_item_operation(&self) -> bool {
        matches!(
            self,
            Self::AddItem
                | Self::AddItems
                | Self::DeleteItems
                | Self::DeleteAppendItems
                | Self::InsertItemsAfter
                | Self::SwapItems
        )
    }
    
    /// Check if this is a group operation
    pub fn is_group_operation(&self) -> bool {
        matches!(self, Self::CreateGroup | Self::DeleteGroup)
    }
}

/// Item types in v6 CRDT format
/// 
/// Each item in a document has a type that determines its rendering
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum ItemType {
    /// Line/stroke item with points
    Line = 0,
    /// Text item (typed text)
    Text = 1,
    /// Glyph range (handwriting converted to text)
    GlyphRange = 2,
    /// Group container
    Group = 3,
    /// Unknown/extension item
    Unknown = 255,
}

impl ItemType {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Line,
            1 => Self::Text,
            2 => Self::GlyphRange,
            3 => Self::Group,
            _ => Self::Unknown,
        }
    }
}

/// Author ID for tracking who made each edit
/// 
/// Required for CRDT conflict resolution and collaboration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorId {
    /// 16-byte author identifier
    pub id: [u8; 16],
}

impl AuthorId {
    pub fn new(id: [u8; 16]) -> Self {
        Self { id }
    }
    
    /// Parse from hex string
    pub fn from_hex(s: &str) -> Option<Self> {
        if s.len() != 32 {
            return None;
        }
        let mut id = [0u8; 16];
        for i in 0..16 {
            id[i] = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
        }
        Some(Self { id })
    }
    
    /// Convert to hex string
    pub fn to_hex(&self) -> String {
        self.id.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

impl std::fmt::Display for AuthorId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}
