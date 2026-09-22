//! .rm file parser

use std::io::{Read, Cursor};
use byteorder::{LittleEndian, ReadBytesExt};
use remarkable_core::{Stroke, Point, PenType, Color, Page};
use crate::{LinesError, HEADER_SIZE, HEADER_V6, VERSION_6};
use crate::block::{BlockType, BlockHeader};

/// Parser for .rm files
pub struct LinesParser {
    data: Vec<u8>,
    offset: usize,
    version: u32,
}

impl LinesParser {
    /// Create a new parser from raw bytes
    pub fn new(data: Vec<u8>) -> Result<Self, LinesError> {
        let mut parser = Self {
            data,
            offset: 0,
            version: 0,
        };
        parser.parse_header()?;
        Ok(parser)
    }
    
    /// Parse from file
    pub fn from_file(path: &std::path::Path) -> Result<Self, LinesError> {
        let data = std::fs::read(path)?;
        Self::new(data)
    }
    
    fn parse_header(&mut self) -> Result<(), LinesError> {
        if self.data.len() < HEADER_SIZE {
            return Err(LinesError::InvalidHeader);
        }
        
        // Check for version 6 header
        let header = &self.data[..HEADER_V6.len()];
        if header == HEADER_V6 {
            self.version = VERSION_6;
            self.offset = HEADER_SIZE;
            return Ok(());
        }
        
        // Try to parse version from header
        let header_str = std::str::from_utf8(&self.data[..HEADER_SIZE])
            .map_err(|_| LinesError::InvalidHeader)?
            .trim_end_matches('\0');
        
        if let Some(version_str) = header_str.strip_prefix("reMarkable .lines file, version=") {
            self.version = version_str.parse()
                .map_err(|_| LinesError::InvalidHeader)?;
            self.offset = HEADER_SIZE;
            Ok(())
        } else {
            Err(LinesError::InvalidHeader)
        }
    }
    
    /// Get the file version
    pub fn version(&self) -> u32 {
        self.version
    }
    
    /// Parse all strokes from the file
    pub fn parse_strokes(&mut self) -> Result<Vec<Stroke>, LinesError> {
        let mut strokes = Vec::new();
        
        while self.offset < self.data.len() {
            if self.offset + BlockHeader::SIZE > self.data.len() {
                break;
            }
            
            let header = self.read_block_header()?;
            let block_end = self.offset + header.length as usize;
            
            if block_end > self.data.len() {
                return Err(LinesError::UnexpectedEof);
            }
            
            if header.block_type == BlockType::SceneLineItem {
                if let Ok(stroke) = self.parse_stroke(&header) {
                    strokes.push(stroke);
                }
            }
            
            self.offset = block_end;
        }
        
        Ok(strokes)
    }
    
    fn read_block_header(&mut self) -> Result<BlockHeader, LinesError> {
        let mut cursor = Cursor::new(&self.data[self.offset..]);
        
        let length = cursor.read_u32::<LittleEndian>()
            .map_err(|_| LinesError::UnexpectedEof)?;
        let unknown = cursor.read_u8()
            .map_err(|_| LinesError::UnexpectedEof)?;
        let min_version = cursor.read_u8()
            .map_err(|_| LinesError::UnexpectedEof)?;
        let current_version = cursor.read_u8()
            .map_err(|_| LinesError::UnexpectedEof)?;
        let block_type = cursor.read_u8()
            .map_err(|_| LinesError::UnexpectedEof)?;
        
        self.offset += BlockHeader::SIZE;
        
        Ok(BlockHeader {
            length,
            unknown,
            min_version,
            current_version,
            block_type: BlockType::from_u8(block_type),
        })
    }
    
    fn parse_stroke(&mut self, header: &BlockHeader) -> Result<Stroke, LinesError> {
        let start = self.offset;
        let mut cursor = Cursor::new(&self.data[start..]);
        
        // Read pen type and color (simplified - actual format uses tagged fields)
        let pen_type = cursor.read_u32::<LittleEndian>().unwrap_or(0);
        let color = cursor.read_u32::<LittleEndian>().unwrap_or(0);
        let _unknown = cursor.read_f32::<LittleEndian>().unwrap_or(0.0);
        let base_width = cursor.read_f32::<LittleEndian>().unwrap_or(2.0);
        let point_count = cursor.read_u32::<LittleEndian>().unwrap_or(0);
        
        let mut stroke = Stroke::new(
            PenType::from_u32(pen_type),
            Color::from_u32(color),
            base_width,
        );
        
        // Parse points
        let point_start = start + 20; // After header fields
        for i in 0..point_count as usize {
            let point_offset = point_start + i * Point::SIZE_V6;
            if point_offset + Point::SIZE_V6 > self.data.len() {
                break;
            }
            
            let mut pcursor = Cursor::new(&self.data[point_offset..]);
            let point = Point {
                x: pcursor.read_f32::<LittleEndian>().unwrap_or(0.0),
                y: pcursor.read_f32::<LittleEndian>().unwrap_or(0.0),
                speed: pcursor.read_u16::<LittleEndian>().unwrap_or(0),
                width: pcursor.read_u16::<LittleEndian>().unwrap_or(0),
                direction: pcursor.read_u8().unwrap_or(0),
                pressure: pcursor.read_u8().unwrap_or(0),
            };
            stroke.points.push(point);
        }
        
        Ok(stroke)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_header_parsing() {
        let mut data = vec![0u8; 100];
        data[..HEADER_V6.len()].copy_from_slice(HEADER_V6);
        
        let parser = LinesParser::new(data).unwrap();
        assert_eq!(parser.version(), VERSION_6);
    }
}
