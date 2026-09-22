//! CRDT (Conflict-free Replicated Data Type) operation traits
//!
//! reMarkable uses Lamport timestamps and operation-based CRDTs
//! for conflict-free document synchronization. This module provides:
//!
//! - Timestamp types for causal ordering
//! - Operation type classification
//! - Scene mutation interface
//!
//! # CRDT Model
//!
//! The v6 format uses 14 operation types covering:
//! - Item operations (add, delete, insert, swap)
//! - Group operations (create, delete)
//! - Layer operations (add, delete, move, merge, rename, visibility)

use std::fmt;
use std::cmp::Ordering;

/// Lamport timestamp for causal ordering
///
/// Combines a logical clock (counter) with a replica ID to ensure
/// total ordering even when counters are equal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LamportTimestamp {
    /// Monotonic logical clock
    pub counter: u64,
    /// Replica/device identifier
    pub replica: u64,
}

impl LamportTimestamp {
    /// Create a new timestamp
    pub const fn new(counter: u64, replica: u64) -> Self {
        Self { counter, replica }
    }
    
    /// Create initial timestamp for a replica
    pub const fn initial(replica: u64) -> Self {
        Self { counter: 0, replica }
    }
    
    /// Increment the counter (local operation)
    pub fn tick(&mut self) -> Self {
        self.counter += 1;
        *self
    }
    
    /// Update for a received message (merge + increment)
    pub fn receive(&mut self, other: &Self) -> Self {
        self.counter = self.counter.max(other.counter) + 1;
        *self
    }
    
    /// Parse from "replica:counter" string format
    pub fn parse(s: &str) -> Option<Self> {
        let mut parts = s.split(':');
        let replica = parts.next()?.parse().ok()?;
        let counter = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None; // Extra parts
        }
        Some(Self { counter, replica })
    }
}

impl Ord for LamportTimestamp {
    fn cmp(&self, other: &Self) -> Ordering {
        // Higher counter wins, ties broken by replica ID
        self.counter.cmp(&other.counter)
            .then_with(|| self.replica.cmp(&other.replica))
    }
}

impl PartialOrd for LamportTimestamp {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for LamportTimestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.replica, self.counter)
    }
}

impl Default for LamportTimestamp {
    fn default() -> Self {
        Self::new(0, 0)
    }
}

/// Author UUID for edit attribution
///
/// Each collaborating device has a unique 128-bit identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AuthorUuid {
    pub bytes: [u8; 16],
}

impl AuthorUuid {
    /// Create from raw bytes
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self { bytes }
    }
    
    /// Create a zero UUID (local/anonymous)
    pub const fn zero() -> Self {
        Self { bytes: [0; 16] }
    }
    
    /// Parse from hex string (32 characters)
    pub fn from_hex(s: &str) -> Option<Self> {
        if s.len() != 32 {
            return None;
        }
        let mut bytes = [0u8; 16];
        for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
            let hex = std::str::from_utf8(chunk).ok()?;
            bytes[i] = u8::from_str_radix(hex, 16).ok()?;
        }
        Some(Self { bytes })
    }
    
    /// Convert to hex string
    pub fn to_hex(&self) -> String {
        self.bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }
    
    /// Parse from standard UUID format (with dashes)
    pub fn from_uuid_str(s: &str) -> Option<Self> {
        let hex: String = s.chars().filter(|c| *c != '-').collect();
        Self::from_hex(&hex)
    }
}

impl fmt::Display for AuthorUuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Standard UUID format: 8-4-4-4-12
        let hex = self.to_hex();
        write!(
            f,
            "{}-{}-{}-{}-{}",
            &hex[0..8],
            &hex[8..12],
            &hex[12..16],
            &hex[16..20],
            &hex[20..32]
        )
    }
}

impl Default for AuthorUuid {
    fn default() -> Self {
        Self::zero()
    }
}

/// CRDT operation kinds
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OpKind {
    // Item operations (0-5)
    /// Add a single item (stroke, text, etc.)
    AddItem,
    /// Add multiple items atomically
    AddItems,
    /// Delete items by ID
    DeleteItems,
    /// Delete then append items atomically
    DeleteAppendItems,
    /// Insert items after a specific item
    InsertItemsAfter,
    /// Swap positions of items
    SwapItems,
    
    // Group operations (6-7)
    /// Create a new group/container
    CreateGroup,
    /// Delete a group
    DeleteGroup,
    
    // Layer operations (8-13)
    /// Add a new layer
    AddLayer,
    /// Delete a layer
    DeleteLayer,
    /// Move layer to different position
    MoveLayer,
    /// Merge layer down (combine with layer below)
    MergeLayerDown,
    /// Set layer name
    SetLayerName,
    /// Set layer visibility
    SetLayerVisible,
}

impl OpKind {
    /// Create from raw operation type byte
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
    
    /// Get raw operation type byte
    pub const fn to_u8(&self) -> u8 {
        match self {
            Self::AddItem => 0,
            Self::AddItems => 1,
            Self::DeleteItems => 2,
            Self::DeleteAppendItems => 3,
            Self::InsertItemsAfter => 4,
            Self::SwapItems => 5,
            Self::CreateGroup => 6,
            Self::DeleteGroup => 7,
            Self::AddLayer => 8,
            Self::DeleteLayer => 9,
            Self::MoveLayer => 10,
            Self::MergeLayerDown => 11,
            Self::SetLayerName => 12,
            Self::SetLayerVisible => 13,
        }
    }
    
    /// Check if this is an item-level operation
    pub const fn is_item_op(&self) -> bool {
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
    
    /// Check if this is a group-level operation
    pub const fn is_group_op(&self) -> bool {
        matches!(self, Self::CreateGroup | Self::DeleteGroup)
    }
    
    /// Check if this is a layer-level operation
    pub const fn is_layer_op(&self) -> bool {
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
}

impl fmt::Display for OpKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::AddItem => "AddItem",
            Self::AddItems => "AddItems",
            Self::DeleteItems => "DeleteItems",
            Self::DeleteAppendItems => "DeleteAppendItems",
            Self::InsertItemsAfter => "InsertItemsAfter",
            Self::SwapItems => "SwapItems",
            Self::CreateGroup => "CreateGroup",
            Self::DeleteGroup => "DeleteGroup",
            Self::AddLayer => "AddLayer",
            Self::DeleteLayer => "DeleteLayer",
            Self::MoveLayer => "MoveLayer",
            Self::MergeLayerDown => "MergeLayerDown",
            Self::SetLayerName => "SetLayerName",
            Self::SetLayerVisible => "SetLayerVisible",
        };
        write!(f, "{name}")
    }
}

/// Error for CRDT operations
#[derive(Debug, thiserror::Error)]
pub enum CrdtError {
    #[error("invalid operation type: {0}")]
    InvalidOpType(u8),
    
    #[error("operation conflict: {0}")]
    Conflict(String),
    
    #[error("target not found: {0}")]
    TargetNotFound(String),
    
    #[error("operation requires author ID")]
    MissingAuthor,
}

/// Scene state that operations modify
///
/// This is a placeholder for the actual scene implementation.
/// In practice, this would be the document's in-memory representation.
pub struct Scene {
    /// Placeholder field
    _private: (),
}

impl Scene {
    /// Create an empty scene
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for Scene {
    fn default() -> Self {
        Self::new()
    }
}

/// Core CRDT operation trait
///
/// Implementations represent specific operation instances with full
/// metadata for replication and conflict resolution.
///
/// # Object Safety
///
/// This trait is object-safe. Use `Box<dyn CrdtOp>` for heterogeneous
/// operation storage.
pub trait CrdtOp: Send + Sync {
    /// Get the operation kind
    fn op_kind(&self) -> OpKind;
    
    /// Get the Lamport timestamp
    fn timestamp(&self) -> LamportTimestamp;
    
    /// Get the author who created this operation
    fn author(&self) -> AuthorUuid;
    
    /// Apply this operation to a scene
    fn apply(&self, scene: &mut Scene) -> Result<(), CrdtError>;
    
    /// Get the inverse operation (for undo)
    ///
    /// Returns None if the operation is not invertible.
    fn inverse(&self) -> Option<Box<dyn CrdtOp>> {
        None
    }
    
    /// Check if this operation conflicts with another
    fn conflicts_with(&self, other: &dyn CrdtOp) -> bool {
        // Default: operations by the same author at same time conflict
        self.author() == other.author() && self.timestamp() == other.timestamp()
    }
}

/// Item identifier in CRDT operations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ItemId {
    /// Author who created the item
    pub author: u64,
    /// Sequence number within author's items
    pub sequence: u64,
}

impl ItemId {
    pub const fn new(author: u64, sequence: u64) -> Self {
        Self { author, sequence }
    }
    
    pub const fn zero() -> Self {
        Self { author: 0, sequence: 0 }
    }
}

impl fmt::Display for ItemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.author, self.sequence)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_lamport_ordering() {
        let t1 = LamportTimestamp::new(1, 100);
        let t2 = LamportTimestamp::new(2, 100);
        let t3 = LamportTimestamp::new(1, 200);
        
        assert!(t1 < t2); // Higher counter wins
        assert!(t1 < t3); // Same counter, higher replica wins
        assert!(t2 > t3); // Counter 2 > counter 1
    }
    
    #[test]
    fn test_lamport_receive() {
        let mut local = LamportTimestamp::new(5, 1);
        let remote = LamportTimestamp::new(10, 2);
        local.receive(&remote);
        assert_eq!(local.counter, 11); // max(5, 10) + 1
    }
    
    #[test]
    fn test_author_uuid_roundtrip() {
        let uuid = AuthorUuid::from_hex("0123456789abcdef0123456789abcdef").unwrap();
        assert_eq!(uuid.to_hex(), "0123456789abcdef0123456789abcdef");
        
        let formatted = uuid.to_string();
        assert_eq!(formatted, "01234567-89ab-cdef-0123-456789abcdef");
    }
    
    #[test]
    fn test_op_kind_categories() {
        assert!(OpKind::AddItem.is_item_op());
        assert!(OpKind::CreateGroup.is_group_op());
        assert!(OpKind::AddLayer.is_layer_op());
        
        assert!(!OpKind::AddItem.is_layer_op());
        assert!(!OpKind::CreateGroup.is_item_op());
    }
}
