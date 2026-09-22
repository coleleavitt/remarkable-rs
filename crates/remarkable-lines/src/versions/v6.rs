//! Version 6 .rm parser - CRDT-based tagged block format
//!
//! This is the modern format used from firmware 3.0+.
//! It uses tagged blocks with CRDT (Conflict-free Replicated Data Types).
//!
//! SceneLineItemBlock (type 0x05) structure:
//!   - Tag 1: parent_id (CrdtId)
//!   - Tag 2: item_id (CrdtId)
//!   - Tag 3: left_id (CrdtId)
//!   - Tag 4: right_id (CrdtId)
//!   - Tag 5: deleted_length (u32)
//!   - Tag 6: subblock containing line data:
//!     - item_type (u8) = 0x03 for lines
//!     - Tag 1: tool (u32)
//!     - Tag 2: color (u32)
//!     - Tag 3: thickness_scale (f64)
//!     - Tag 4: starting_length (f32)
//!     - Tag 5: points subblock (raw point data)
//!     - Tag 6: timestamp (CrdtId)
//!
//! Point format depends on block version:
//!   Block v1: 24 bytes - x, y, speed, direction, width, pressure (all f32)
//!   Block v2: 14 bytes - x, y (f32), speed, width (u16), direction, pressure (u8)

use std::io::Cursor;
use byteorder::{LittleEndian, ReadBytesExt};
use remarkable_core::{Stroke, Point, PenType, Color, CrdtId};
use crate::LinesError;
use crate::versions::{RmParser, HEADER_V6, HEADER_SIZE};

/// Block types in v6 format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum BlockType {
    MigrationInfo = 0x00,
    SceneTree = 0x01,
    TreeNode = 0x02,
    SceneGroupItem = 0x03,
    SceneItem = 0x04,
    SceneLineItem = 0x05,
    SceneTextItem = 0x06,
    PageInfo = 0x09,
    SceneGlyphItem = 0x0A,
    AuthorIds = 0x0D,
    Unknown(u8),
}

impl From<u8> for BlockType {
    fn from(v: u8) -> Self {
        match v {
            0x00 => BlockType::MigrationInfo,
            0x01 => BlockType::SceneTree,
            0x02 => BlockType::TreeNode,
            0x03 => BlockType::SceneGroupItem,
            0x04 => BlockType::SceneItem,
            0x05 => BlockType::SceneLineItem,
            0x06 => BlockType::SceneTextItem,
            0x09 => BlockType::PageInfo,
            0x0A => BlockType::SceneGlyphItem,
            0x0D => BlockType::AuthorIds,
            other => BlockType::Unknown(other),
        }
    }
}

/// Tag types in tagged block format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TagType {
    Byte1 = 0x1,
    Byte4 = 0x4,
    Byte8 = 0x8,
    Length4 = 0xC,
    ID = 0xF,
}

/// Block header information
#[derive(Debug)]
pub struct BlockInfo {
    pub block_type: BlockType,
    pub min_version: u8,
    pub current_version: u8,
    pub offset: usize,
    pub size: usize,
}

/// Parser for .rm format version 6
pub struct V6Parser {
    data: Vec<u8>,
    cursor: usize,
}

impl V6Parser {
    pub fn new(data: Vec<u8>) -> Result<Self, LinesError> {
        if data.len() < HEADER_SIZE {
            return Err(LinesError::InvalidHeader);
        }
        if &data[..HEADER_SIZE] != HEADER_V6 {
            return Err(LinesError::InvalidHeader);
        }
        Ok(Self { 
            data, 
            cursor: HEADER_SIZE,
        })
    }
    
    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.cursor)
    }
    
    fn read_u8(&mut self) -> Result<u8, LinesError> {
        if self.cursor >= self.data.len() {
            return Err(LinesError::UnexpectedEof);
        }
        let val = self.data[self.cursor];
        self.cursor += 1;
        Ok(val)
    }
    
    fn read_u16(&mut self) -> Result<u16, LinesError> {
        if self.cursor + 2 > self.data.len() {
            return Err(LinesError::UnexpectedEof);
        }
        let mut cursor = Cursor::new(&self.data[self.cursor..self.cursor + 2]);
        let val = cursor.read_u16::<LittleEndian>().map_err(|_| LinesError::UnexpectedEof)?;
        self.cursor += 2;
        Ok(val)
    }
    
    fn read_u32(&mut self) -> Result<u32, LinesError> {
        if self.cursor + 4 > self.data.len() {
            return Err(LinesError::UnexpectedEof);
        }
        let mut cursor = Cursor::new(&self.data[self.cursor..self.cursor + 4]);
        let val = cursor.read_u32::<LittleEndian>().map_err(|_| LinesError::UnexpectedEof)?;
        self.cursor += 4;
        Ok(val)
    }
    
    fn read_f32(&mut self) -> Result<f32, LinesError> {
        if self.cursor + 4 > self.data.len() {
            return Err(LinesError::UnexpectedEof);
        }
        let mut cursor = Cursor::new(&self.data[self.cursor..self.cursor + 4]);
        let val = cursor.read_f32::<LittleEndian>().map_err(|_| LinesError::UnexpectedEof)?;
        self.cursor += 4;
        Ok(val)
    }
    
    fn read_f64(&mut self) -> Result<f64, LinesError> {
        if self.cursor + 8 > self.data.len() {
            return Err(LinesError::UnexpectedEof);
        }
        let mut cursor = Cursor::new(&self.data[self.cursor..self.cursor + 8]);
        let val = cursor.read_f64::<LittleEndian>().map_err(|_| LinesError::UnexpectedEof)?;
        self.cursor += 8;
        Ok(val)
    }
    
    /// Read a variable-length unsigned integer (LEB128)
    fn read_varuint(&mut self) -> Result<u64, LinesError> {
        let mut result: u64 = 0;
        let mut shift = 0;
        loop {
            let byte = self.read_u8()?;
            result |= ((byte & 0x7F) as u64) << shift;
            shift += 7;
            if (byte & 0x80) == 0 {
                break;
            }
            if shift > 63 {
                return Err(LinesError::InvalidData("varuint overflow".into()));
            }
        }
        Ok(result)
    }
    
    /// Read a CRDT ID (part1: u8, part2: varuint)
    fn read_crdt_id(&mut self) -> Result<CrdtId, LinesError> {
        let part1 = self.read_u8()? as u64;
        let part2 = self.read_varuint()?;
        Ok(CrdtId::new(part1, part2))
    }
    
    /// Read a tag and return (index, type)
    fn read_tag(&mut self) -> Result<(u8, u8), LinesError> {
        let tag = self.read_varuint()? as u32;
        let index = (tag >> 4) as u8;
        let tag_type = (tag & 0x0F) as u8;
        Ok((index, tag_type))
    }
    
    /// Peek at the next tag without consuming it
    fn peek_tag(&self) -> Option<(u8, u8)> {
        if self.cursor >= self.data.len() {
            return None;
        }
        
        // Read varuint without advancing cursor
        let mut pos = self.cursor;
        let mut result: u32 = 0;
        let mut shift = 0;
        
        loop {
            if pos >= self.data.len() {
                return None;
            }
            let byte = self.data[pos];
            result |= ((byte & 0x7F) as u32) << shift;
            shift += 7;
            pos += 1;
            if (byte & 0x80) == 0 {
                break;
            }
            if shift > 28 {
                return None;
            }
        }
        
        let index = (result >> 4) as u8;
        let tag_type = (result & 0x0F) as u8;
        Some((index, tag_type))
    }
    
    /// Skip a value based on its tag type
    fn skip_value(&mut self, tag_type: u8) -> Result<(), LinesError> {
        match tag_type {
            0x1 => { self.cursor += 1; } // Byte1
            0x4 => { self.cursor += 4; } // Byte4
            0x8 => { self.cursor += 8; } // Byte8
            0xC => {
                // Length4 - subblock
                let len = self.read_u32()? as usize;
                self.cursor += len;
            }
            0xF => {
                // ID - CrdtId
                self.read_crdt_id()?;
            }
            _ => {
                return Err(LinesError::InvalidData(format!("Unknown tag type: 0x{:x}", tag_type)));
            }
        }
        Ok(())
    }
    
    /// Read a block header
    fn read_block_header(&mut self) -> Result<Option<BlockInfo>, LinesError> {
        if self.remaining() < 8 {
            return Ok(None);
        }
        
        let block_length = self.read_u32()? as usize;
        let _unknown = self.read_u8()?;
        let min_version = self.read_u8()?;
        let current_version = self.read_u8()?;
        let block_type_byte = self.read_u8()?;
        
        let block_type = BlockType::from(block_type_byte);
        
        Ok(Some(BlockInfo {
            block_type,
            min_version,
            current_version,
            offset: self.cursor,
            size: block_length,
        }))
    }
    
    /// Parse a point in block version 1 format (24 bytes)
    fn parse_point_v1(&mut self) -> Result<Point, LinesError> {
        let x = self.read_f32()?;
        let y = self.read_f32()?;
        // Speed stored as float, multiply by 4
        let speed_f = self.read_f32()? * 4.0;
        // Direction stored as float * (2π / 255)
        let direction_f = self.read_f32()?;
        let width_f = self.read_f32()?;
        let pressure_f = self.read_f32()?;
        
        // Convert to compressed format
        let speed = (speed_f * 10.0).clamp(0.0, 65535.0) as u16;
        let width = (width_f * 100.0).clamp(0.0, 65535.0) as u16;
        let direction = ((direction_f * 255.0 / std::f32::consts::TAU).abs() % 256.0) as u8;
        let pressure = (pressure_f.clamp(0.0, 1.0) * 255.0) as u8;
        
        Ok(Point {
            x,
            y,
            speed,
            width,
            direction,
            pressure,
        })
    }
    
    /// Parse a point in block version 2 format (14 bytes)
    fn parse_point_v2(&mut self) -> Result<Point, LinesError> {
        let x = self.read_f32()?;
        let y = self.read_f32()?;
        let speed = self.read_u16()?;
        let width = self.read_u16()?;
        let direction = self.read_u8()?;
        let pressure = self.read_u8()?;
        
        Ok(Point {
            x,
            y,
            speed,
            width,
            direction,
            pressure,
        })
    }
    
    /// Parse a SceneLineItemBlock (type 0x05)
    fn parse_line_block(&mut self, block: &BlockInfo) -> Result<Option<Stroke>, LinesError> {
        let block_end = block.offset + block.size;
        let version = block.current_version;
        
        // SceneItemBlock structure:
        // Tag 1: parent_id (CrdtId)
        // Tag 2: item_id (CrdtId)
        // Tag 3: left_id (CrdtId)
        // Tag 4: right_id (CrdtId)
        // Tag 5: deleted_length (u32)
        // Tag 6: subblock containing the line data
        
        // Read and skip CRDT sequence metadata (tags 1-5)
        while self.cursor < block_end {
            let (index, tag_type) = self.read_tag()?;
            
            if index == 6 && tag_type == TagType::Length4 as u8 {
                // This is the value subblock
                let subblock_len = self.read_u32()? as usize;
                let subblock_end = self.cursor + subblock_len;
                
                // First byte is item_type (0x03 for lines)
                let item_type = self.read_u8()?;
                if item_type != 0x03 {
                    // Not a line, skip
                    self.cursor = subblock_end;
                    self.cursor = block_end;
                    return Ok(None);
                }
                
                // Now parse the line data
                // Tag 1: tool (u32)
                // Tag 2: color (u32)
                // Tag 3: thickness_scale (f64)
                // Tag 4: starting_length (f32)
                // Tag 5: points subblock
                // Tag 6: timestamp (CrdtId)
                
                let mut pen_id = 4; // Default fineliner
                let mut color_id = 0; // Default black
                let mut thickness_scale = 1.0f64;
                let mut points = Vec::new();
                
                while self.cursor < subblock_end {
                    let Some((idx, ttype)) = self.peek_tag() else { break };
                    
                    if idx == 1 && ttype == TagType::Byte4 as u8 {
                        self.read_tag()?;
                        pen_id = self.read_u32()?;
                    } else if idx == 2 && ttype == TagType::Byte4 as u8 {
                        self.read_tag()?;
                        color_id = self.read_u32()?;
                    } else if idx == 3 && ttype == TagType::Byte8 as u8 {
                        self.read_tag()?;
                        thickness_scale = self.read_f64()?;
                    } else if idx == 4 && ttype == TagType::Byte4 as u8 {
                        self.read_tag()?;
                        let _starting_length = self.read_f32()?;
                    } else if idx == 5 && ttype == TagType::Length4 as u8 {
                        self.read_tag()?;
                        let points_len = self.read_u32()? as usize;
                        
                        // Calculate point count based on block version
                        let point_size = if version >= 2 { 14 } else { 24 };
                        
                        if points_len % point_size != 0 {
                            // Invalid point data, skip
                            self.cursor += points_len;
                            continue;
                        }
                        
                        let point_count = points_len / point_size;
                        points = Vec::with_capacity(point_count);
                        
                        for _ in 0..point_count {
                            let point = if version >= 2 {
                                self.parse_point_v2()?
                            } else {
                                self.parse_point_v1()?
                            };
                            points.push(point);
                        }
                    } else if idx == 6 && ttype == TagType::ID as u8 {
                        self.read_tag()?;
                        let _timestamp = self.read_crdt_id()?;
                    } else {
                        // Skip unknown tags
                        self.read_tag()?;
                        self.skip_value(ttype)?;
                    }
                }
                
                // Skip to end of block
                self.cursor = block_end;
                
                if points.is_empty() {
                    return Ok(None);
                }
                
                let pen = PenType::from_u32(pen_id);
                let color = Color::from_u32(color_id);
                
                return Ok(Some(Stroke {
                    pen,
                    color,
                    base_width: thickness_scale as f32,
                    points,
                }));
            } else {
                // Skip non-value tags
                self.skip_value(tag_type)?;
            }
        }
        
        // Skip to end of block
        self.cursor = block_end;
        Ok(None)
    }
}

impl RmParser for V6Parser {
    fn parse_strokes(&mut self) -> Result<Vec<Stroke>, LinesError> {
        self.cursor = HEADER_SIZE;
        let mut strokes = Vec::new();
        
        while self.remaining() > 8 {
            match self.read_block_header()? {
                Some(block) => {
                    match block.block_type {
                        BlockType::SceneLineItem => {
                            if let Some(stroke) = self.parse_line_block(&block)? {
                                strokes.push(stroke);
                            }
                        }
                        _ => {
                            // Skip other block types
                            self.cursor = block.offset + block.size;
                        }
                    }
                }
                None => break,
            }
        }
        
        Ok(strokes)
    }
    
    fn version(&self) -> u32 {
        6
    }
    
    fn point_size(&self) -> usize {
        14 // Default to v2 (block version >= 2)
    }
}
