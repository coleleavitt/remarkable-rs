//! Block types for .rm file format

/// Block type identifiers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum BlockType {
    /// Unknown block
    Unknown = 0,
    /// Scene tree info
    SceneTreeInfo = 1,
    /// Root text block
    RootText = 2,
    /// Tree node (layer)
    TreeNode = 3,
    /// Scene line item (strokes)
    SceneLineItem = 4,
    /// Scene glyph item (text)
    SceneGlyphItem = 5,
    /// Group item
    GroupItem = 6,
    /// Page info
    PageInfo = 7,
    /// Glyph range
    GlyphRange = 8,
    /// Root item
    RootItem = 9,
    /// Scene group item reference
    SceneGroupItemRef = 10,
    /// Scene line item reference
    SceneLineItemRef = 11,
    /// Scene text item
    SceneTextItem = 12,
    /// Anchor position item
    AnchorPositionItem = 13,
}

impl BlockType {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::SceneTreeInfo,
            2 => Self::RootText,
            3 => Self::TreeNode,
            4 => Self::SceneLineItem,
            5 => Self::SceneGlyphItem,
            6 => Self::GroupItem,
            7 => Self::PageInfo,
            8 => Self::GlyphRange,
            9 => Self::RootItem,
            10 => Self::SceneGroupItemRef,
            11 => Self::SceneLineItemRef,
            12 => Self::SceneTextItem,
            13 => Self::AnchorPositionItem,
            _ => Self::Unknown,
        }
    }
}

/// Block header (8 bytes)
#[derive(Debug, Clone)]
pub struct BlockHeader {
    /// Block data length
    pub length: u32,
    /// Unknown field
    pub unknown: u8,
    /// Minimum version required
    pub min_version: u8,
    /// Current version
    pub current_version: u8,
    /// Block type
    pub block_type: BlockType,
}

impl BlockHeader {
    pub const SIZE: usize = 8;
}
