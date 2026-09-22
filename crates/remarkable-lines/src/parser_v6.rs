//! V6 tagged block parser for .rm files
//! 
//! Based on the rmscene Python library by Rick Lupton and ddvk's Go reader.
//! V6 format uses protobuf-style tagged fields with LEB128 varint encoding.

use std::io::{Read, Cursor, Seek, SeekFrom};
use byteorder::{LittleEndian, ReadBytesExt};
use remarkable_core::{Stroke, Point, PenType, Color};
use crate::LinesError;

/// Header for version 6 files
pub const HEADER_V6: &[u8; 43] = b"reMarkable .lines file, version=6          ";

/// Tag types for tagged fields (lower 4 bits of tag)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TagType {
    /// 1 byte value
    Byte1 = 0x1,
    /// 4 byte value
    Byte4 = 0x4,
    /// 8 byte value  
    Byte8 = 0x8,
    /// Length-prefixed block (4 byte length)
    Length4 = 0xC,
    /// CRDT ID (special format)
    Id = 0xF,
}

impl TagType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0x1 => Some(Self::Byte1),
            0x4 => Some(Self::Byte4),
            0x8 => Some(Self::Byte8),
            0xC => Some(Self::Length4),
            0xF => Some(Self::Id),
            _ => None,
        }
    }
}

/// CRDT identifier (replica, counter)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CrdtId {
    pub part1: u8,
    pub part2: u64,
}

impl CrdtId {
    pub fn new(part1: u8, part2: u64) -> Self {
        Self { part1, part2 }
    }
}

/// Block types in v6 format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum BlockType {
    /// Migration info
    MigrationInfo = 0,
    /// Scene tree  
    SceneTree = 1,
    /// Tree node (layer)
    TreeNode = 2,
    /// Scene glyph item
    SceneGlyphItem = 3,
    /// Scene group item
    SceneGroupItem = 4,
    /// Scene line item (stroke)
    SceneLineItem = 5,
    /// Scene text item
    SceneTextItem = 6,
    /// Root text block
    RootText = 7,
    /// Scene tombstone
    SceneTombstone = 8,
    /// Author IDs block
    AuthorIds = 9,
    /// Page info
    PageInfo = 10,
    /// Scene info
    SceneInfo = 13,
    /// Unknown
    Unknown(u8),
}

impl BlockType {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::MigrationInfo,
            1 => Self::SceneTree,
            2 => Self::TreeNode,
            3 => Self::SceneGlyphItem,
            4 => Self::SceneGroupItem,
            5 => Self::SceneLineItem,
            6 => Self::SceneTextItem,
            7 => Self::RootText,
            8 => Self::SceneTombstone,
            9 => Self::AuthorIds,
            10 => Self::PageInfo,
            13 => Self::SceneInfo,
            _ => Self::Unknown(v),
        }
    }
}

/// Main block header
#[derive(Debug, Clone)]
pub struct BlockHeader {
    pub length: u32,
    pub unknown: u8,
    pub min_version: u8,
    pub current_version: u8,
    pub block_type: BlockType,
}

impl BlockHeader {
    pub const SIZE: usize = 8;
}

/// Reader for v6 tagged block format
pub struct TaggedBlockReader<R: Read + Seek> {
    reader: R,
    position: u64,
}

impl<R: Read + Seek> TaggedBlockReader<R> {
    pub fn new(reader: R) -> Self {
        Self { reader, position: 0 }
    }
    
    /// Read file header, returns version number
    pub fn read_header(&mut self) -> Result<u32, LinesError> {
        let mut header = [0u8; 43];
        self.reader.read_exact(&mut header)?;
        
        if &header == HEADER_V6 {
            self.position = 43;
            Ok(6)
        } else {
            Err(LinesError::InvalidHeader)
        }
    }
    
    /// Read a varint (LEB128 encoded)
    pub fn read_varint(&mut self) -> Result<u64, LinesError> {
        let mut result: u64 = 0;
        let mut shift = 0;
        
        loop {
            let mut buf = [0u8; 1];
            self.reader.read_exact(&mut buf)?;
            self.position += 1;
            
            let byte = buf[0];
            result |= ((byte & 0x7F) as u64) << shift;
            shift += 7;
            
            if byte & 0x80 == 0 {
                break;
            }
            
            if shift >= 64 {
                return Err(LinesError::InvalidData("varint overflow".into()));
            }
        }
        
        Ok(result)
    }
    
    /// Read a tag (index, tag_type)
    pub fn read_tag(&mut self) -> Result<(u32, TagType), LinesError> {
        let x = self.read_varint()?;
        let index = (x >> 4) as u32;
        let tag_type_raw = (x & 0xF) as u8;
        
        let tag_type = TagType::from_u8(tag_type_raw)
            .ok_or_else(|| LinesError::InvalidData(format!("unknown tag type: 0x{:X}", tag_type_raw)))?;
        
        Ok((index, tag_type))
    }
    
    /// Read a CRDT ID
    pub fn read_crdt_id(&mut self) -> Result<CrdtId, LinesError> {
        let mut buf = [0u8; 1];
        self.reader.read_exact(&mut buf)?;
        self.position += 1;
        let part1 = buf[0];
        let part2 = self.read_varint()?;
        Ok(CrdtId::new(part1, part2))
    }
    
    /// Read a tagged CRDT ID
    pub fn read_tagged_id(&mut self, expected_index: u32) -> Result<CrdtId, LinesError> {
        let (index, tag_type) = self.read_tag()?;
        if index != expected_index {
            return Err(LinesError::InvalidData(format!("expected tag index {}, got {}", expected_index, index)));
        }
        if tag_type != TagType::Id {
            return Err(LinesError::InvalidData(format!("expected ID tag type, got {:?}", tag_type)));
        }
        self.read_crdt_id()
    }
    
    /// Read a tagged u32
    pub fn read_tagged_u32(&mut self, expected_index: u32) -> Result<u32, LinesError> {
        let (index, tag_type) = self.read_tag()?;
        if index != expected_index {
            return Err(LinesError::InvalidData(format!("expected tag index {}, got {}", expected_index, index)));
        }
        if tag_type != TagType::Byte4 {
            return Err(LinesError::InvalidData(format!("expected Byte4 tag type, got {:?}", tag_type)));
        }
        let value = self.reader.read_u32::<LittleEndian>()?;
        self.position += 4;
        Ok(value)
    }
    
    /// Read a tagged f32
    pub fn read_tagged_f32(&mut self, expected_index: u32) -> Result<f32, LinesError> {
        let (index, tag_type) = self.read_tag()?;
        if index != expected_index {
            return Err(LinesError::InvalidData(format!("expected tag index {}, got {}", expected_index, index)));
        }
        if tag_type != TagType::Byte4 {
            return Err(LinesError::InvalidData(format!("expected Byte4 tag type, got {:?}", tag_type)));
        }
        let value = self.reader.read_f32::<LittleEndian>()?;
        self.position += 4;
        Ok(value)
    }
    
    /// Read a tagged f64
    pub fn read_tagged_f64(&mut self, expected_index: u32) -> Result<f64, LinesError> {
        let (index, tag_type) = self.read_tag()?;
        if index != expected_index {
            return Err(LinesError::InvalidData(format!("expected tag index {}, got {}", expected_index, index)));
        }
        if tag_type != TagType::Byte8 {
            return Err(LinesError::InvalidData(format!("expected Byte8 tag type, got {:?}", tag_type)));
        }
        let value = self.reader.read_f64::<LittleEndian>()?;
        self.position += 8;
        Ok(value)
    }
    
    /// Read a tagged length-prefixed subblock
    pub fn read_tagged_subblock(&mut self, expected_index: u32) -> Result<Vec<u8>, LinesError> {
        let (index, tag_type) = self.read_tag()?;
        if index != expected_index {
            return Err(LinesError::InvalidData(format!("expected tag index {}, got {}", expected_index, index)));
        }
        if tag_type != TagType::Length4 {
            return Err(LinesError::InvalidData(format!("expected Length4 tag type, got {:?}", tag_type)));
        }
        
        let length = self.reader.read_u32::<LittleEndian>()? as usize;
        self.position += 4;
        
        let mut data = vec![0u8; length];
        self.reader.read_exact(&mut data)?;
        self.position += length as u64;
        
        Ok(data)
    }
    
    /// Read main block header
    pub fn read_block_header(&mut self) -> Result<BlockHeader, LinesError> {
        let length = self.reader.read_u32::<LittleEndian>()?;
        let unknown = self.reader.read_u8()?;
        let min_version = self.reader.read_u8()?;
        let current_version = self.reader.read_u8()?;
        let block_type_raw = self.reader.read_u8()?;
        
        self.position += 8;
        
        Ok(BlockHeader {
            length,
            unknown,
            min_version,
            current_version,
            block_type: BlockType::from_u8(block_type_raw),
        })
    }
    
    /// Check if there are more bytes to read
    pub fn has_more(&mut self) -> bool {
        let current = self.reader.stream_position().unwrap_or(0);
        let end = self.reader.seek(SeekFrom::End(0)).unwrap_or(0);
        self.reader.seek(SeekFrom::Start(current)).ok();
        current < end
    }
    
    /// Get current position
    pub fn position(&self) -> u64 {
        self.position
    }
    
    /// Skip n bytes
    pub fn skip(&mut self, n: u64) -> Result<(), LinesError> {
        self.reader.seek(SeekFrom::Current(n as i64))?;
        self.position += n;
        Ok(())
    }
}

/// Point size in bytes for v6 format (version 2 points)
pub const POINT_SIZE_V6: usize = 14;

/// Parse points from raw data (v6 format, version 2 points)
pub fn parse_points_v6(data: &[u8]) -> Result<Vec<Point>, LinesError> {
    if data.len() % POINT_SIZE_V6 != 0 {
        return Err(LinesError::InvalidData(format!(
            "point data size {} is not a multiple of {}",
            data.len(), POINT_SIZE_V6
        )));
    }
    
    let num_points = data.len() / POINT_SIZE_V6;
    let mut points = Vec::with_capacity(num_points);
    
    for i in 0..num_points {
        let offset = i * POINT_SIZE_V6;
        let mut cursor = Cursor::new(&data[offset..offset + POINT_SIZE_V6]);
        
        let x = cursor.read_f32::<LittleEndian>()?;
        let y = cursor.read_f32::<LittleEndian>()?;
        let speed = cursor.read_u16::<LittleEndian>()?;
        let width = cursor.read_u16::<LittleEndian>()?;
        let direction = cursor.read_u8()?;
        let pressure = cursor.read_u8()?;
        
        points.push(Point { x, y, speed, width, direction, pressure });
    }
    
    Ok(points)
}

/// Parse a Line/Stroke from tagged block reader
pub fn parse_line_item<R: Read + Seek>(reader: &mut TaggedBlockReader<R>) -> Result<Stroke, LinesError> {
    // Tag 1: tool_id (int)
    let tool_id = reader.read_tagged_u32(1)?;
    let pen = PenType::from_u32(tool_id);
    
    // Tag 2: color_id (int)
    let color_id = reader.read_tagged_u32(2)?;
    let color = Color::from_u32(color_id);
    
    // Tag 3: thickness_scale (double)
    let thickness_scale = reader.read_tagged_f64(3)?;
    
    // Tag 4: starting_length (float)
    let _starting_length = reader.read_tagged_f32(4)?;
    
    // Tag 5: points data (subblock)
    let point_data = reader.read_tagged_subblock(5)?;
    let points = parse_points_v6(&point_data)?;
    
    // Tag 6: timestamp (CrdtId) - read but not used
    let _timestamp = reader.read_tagged_id(6)?;
    
    // Optional: Tag 7: move_id, Tag 8: color_rgba
    // We skip these for now as they're optional
    
    Ok(Stroke {
        pen,
        color,
        base_width: thickness_scale as f32,
        points,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_varint() {
        // Test single byte varint
        let data = vec![0x05u8];
        let mut reader = TaggedBlockReader::new(Cursor::new(data));
        assert_eq!(reader.read_varint().unwrap(), 5);
        
        // Test multi-byte varint (300 = 0xAC 0x02)
        let data = vec![0xAC, 0x02];
        let mut reader = TaggedBlockReader::new(Cursor::new(data));
        assert_eq!(reader.read_varint().unwrap(), 300);
    }
    
    #[test]
    fn test_tag_parsing() {
        // Tag with index=1, type=Byte4 (0x4) -> (1 << 4) | 4 = 0x14
        let data = vec![0x14u8];
        let mut reader = TaggedBlockReader::new(Cursor::new(data));
        let (index, tag_type) = reader.read_tag().unwrap();
        assert_eq!(index, 1);
        assert_eq!(tag_type, TagType::Byte4);
    }
    
    #[test]
    fn test_point_parsing() {
        // Create sample point data
        let mut data = Vec::new();
        // x = 100.0
        data.extend_from_slice(&100.0f32.to_le_bytes());
        // y = 200.0
        data.extend_from_slice(&200.0f32.to_le_bytes());
        // speed = 1000
        data.extend_from_slice(&1000u16.to_le_bytes());
        // width = 50
        data.extend_from_slice(&50u16.to_le_bytes());
        // direction = 128
        data.push(128);
        // pressure = 200
        data.push(200);
        
        let points = parse_points_v6(&data).unwrap();
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].x, 100.0);
        assert_eq!(points[0].y, 200.0);
        assert_eq!(points[0].speed, 1000);
        assert_eq!(points[0].width, 50);
        assert_eq!(points[0].direction, 128);
        assert_eq!(points[0].pressure, 200);
    }
}
