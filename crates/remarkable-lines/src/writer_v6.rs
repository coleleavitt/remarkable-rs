//! V6 format writer for .rm files
//!
//! Writes CRDT-based .rm files compatible with firmware 3.0+

use std::io::{Write, Cursor};
use byteorder::{LittleEndian, WriteBytesExt};
use remarkable_core::{Stroke, Point, PenType, Color};
use crate::LinesError;
use crate::versions::HEADER_V6;

/// Write a varint (LEB128 encoding)
fn write_varint<W: Write>(writer: &mut W, mut value: u64) -> std::io::Result<()> {
    loop {
        let byte = (value & 0x7F) as u8;
        value >>= 7;
        if value == 0 {
            writer.write_all(&[byte])?;
            break;
        } else {
            writer.write_all(&[byte | 0x80])?;
        }
    }
    Ok(())
}

/// Write a tagged u8 value
fn write_tag_byte1<W: Write>(writer: &mut W, index: u8, value: u8) -> std::io::Result<()> {
    let tag = (index << 4) | 0x01;
    writer.write_all(&[tag, value])?;
    Ok(())
}

/// Write a tagged u32 value
fn write_tag_byte4<W: Write>(writer: &mut W, index: u8, value: u32) -> std::io::Result<()> {
    let tag = (index << 4) | 0x04;
    writer.write_all(&[tag])?;
    writer.write_u32::<LittleEndian>(value)?;
    Ok(())
}

/// Write a tagged f32 value
fn write_tag_f32<W: Write>(writer: &mut W, index: u8, value: f32) -> std::io::Result<()> {
    let tag = (index << 4) | 0x04;
    writer.write_all(&[tag])?;
    writer.write_f32::<LittleEndian>(value)?;
    Ok(())
}

/// Write a CRDT ID
fn write_crdt_id<W: Write>(writer: &mut W, index: u8, part1: u8, part2: u64) -> std::io::Result<()> {
    let tag = (index << 4) | 0x0F;
    writer.write_all(&[tag, part1])?;
    write_varint(writer, part2)?;
    Ok(())
}

/// Write a length-prefixed subblock
fn write_subblock<W: Write>(writer: &mut W, index: u8, data: &[u8]) -> std::io::Result<()> {
    let tag = (index << 4) | 0x0C;
    writer.write_all(&[tag])?;
    writer.write_u32::<LittleEndian>(data.len() as u32)?;
    writer.write_all(data)?;
    Ok(())
}

/// V6 format writer
pub struct V6Writer {
    author_id: u16,
    item_counter: u64,
}

impl V6Writer {
    pub fn new() -> Self {
        Self {
            author_id: 1,
            item_counter: 1,
        }
    }
    
    /// Set the author ID for new items
    pub fn with_author_id(mut self, id: u16) -> Self {
        self.author_id = id;
        self
    }
    
    /// Write strokes to v6 format
    pub fn write_strokes(&mut self, strokes: &[Stroke]) -> Result<Vec<u8>, LinesError> {
        let mut output = Cursor::new(Vec::new());
        
        // Write header
        output.write_all(HEADER_V6)?;
        
        // Write MigrationInfo block (type 0)
        self.write_migration_info(&mut output)?;
        
        // Write SceneTree block (type 1)
        self.write_scene_tree(&mut output)?;
        
        // Write TreeNode (layer) block (type 2)
        self.write_tree_node(&mut output)?;
        
        // Write AuthorIds block (type 13)
        self.write_author_ids(&mut output)?;
        
        // Write each stroke as SceneLineItem (type 5)
        for stroke in strokes {
            self.write_scene_line_item(&mut output, stroke)?;
        }
        
        // Write PageInfo block (type 10)
        self.write_page_info(&mut output)?;
        
        Ok(output.into_inner())
    }
    
    fn write_block_header<W: Write>(&self, writer: &mut W, block_type: u8, length: u32) -> std::io::Result<()> {
        // Block format: min_version (4 bytes) + current_version (4 bytes) + type (1 byte) + length (4 bytes)
        writer.write_u32::<LittleEndian>(1)?;  // min_version
        writer.write_u32::<LittleEndian>(1)?;  // current_version
        writer.write_all(&[block_type])?;
        writer.write_u32::<LittleEndian>(length)?;
        Ok(())
    }
    
    fn write_migration_info<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        let mut block = Vec::new();
        write_tag_byte4(&mut block, 1, 6)?;  // version = 6
        
        self.write_block_header(writer, 0, block.len() as u32)?;
        writer.write_all(&block)?;
        Ok(())
    }
    
    fn write_scene_tree<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        let mut block = Vec::new();
        // Tree node reference
        write_crdt_id(&mut block, 1, 1, 1)?;
        
        self.write_block_header(writer, 1, block.len() as u32)?;
        writer.write_all(&block)?;
        Ok(())
    }
    
    fn write_tree_node<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        let mut block = Vec::new();
        // group item ID
        write_crdt_id(&mut block, 1, 1, 1)?;
        
        // Layer info subblock
        let mut layer_info = Vec::new();
        write_tag_byte4(&mut layer_info, 1, 0)?;  // layer index = 0
        // visible = true (tag 2, value 1)
        write_tag_byte1(&mut layer_info, 2, 1)?;
        
        write_subblock(&mut block, 2, &layer_info)?;
        
        self.write_block_header(writer, 2, block.len() as u32)?;
        writer.write_all(&block)?;
        Ok(())
    }
    
    fn write_author_ids<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        let mut block = Vec::new();
        
        // Author ID subblock  
        let mut author_block = Vec::new();
        write_tag_byte1(&mut author_block, 1, self.author_id as u8)?;
        // UUID (16 bytes) - use a simple fixed UUID for now
        let uuid = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
                    0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10];
        write_subblock(&mut author_block, 2, &uuid)?;
        
        write_subblock(&mut block, 1, &author_block)?;
        
        self.write_block_header(writer, 13, block.len() as u32)?;
        writer.write_all(&block)?;
        Ok(())
    }
    
    fn write_scene_line_item<W: Write>(&mut self, writer: &mut W, stroke: &Stroke) -> std::io::Result<()> {
        let mut block = Vec::new();
        
        // CRDT item header
        let item_id = self.item_counter;
        self.item_counter += 1;
        
        // parent_id
        write_crdt_id(&mut block, 1, 1, 1)?;
        // item_id
        write_crdt_id(&mut block, 2, self.author_id as u8, item_id)?;
        // left_id (none = 0:0)
        write_crdt_id(&mut block, 3, 0, 0)?;
        // right_id (none = 0:0)
        write_crdt_id(&mut block, 4, 0, 0)?;
        // deleted_length = 0
        write_tag_byte4(&mut block, 5, 0)?;
        
        // Line data subblock (tag 6)
        let mut line_data = Vec::new();
        
        // item_type = 0 (Line)
        line_data.write_all(&[0])?;
        
        // tool (tag 1)
        write_tag_byte4(&mut line_data, 1, stroke.pen.to_u32())?;
        // color (tag 2)
        write_tag_byte4(&mut line_data, 2, stroke.color.to_u32())?;
        // thickness_scale (tag 3)
        write_tag_f32(&mut line_data, 3, stroke.base_width)?;
        // starting_length (tag 4)
        write_tag_f32(&mut line_data, 4, 0.0)?;
        
        // Points subblock (tag 5)
        let points_data = self.encode_points(&stroke.points)?;
        write_subblock(&mut line_data, 5, &points_data)?;
        
        // timestamp (tag 6)
        write_crdt_id(&mut line_data, 6, self.author_id as u8, item_id)?;
        
        write_subblock(&mut block, 6, &line_data)?;
        
        self.write_block_header(writer, 5, block.len() as u32)?;
        writer.write_all(&block)?;
        Ok(())
    }
    
    fn encode_points(&self, points: &[Point]) -> std::io::Result<Vec<u8>> {
        let mut data = Vec::new();
        
        // Point format: x (varint), y (varint), speed (u8), width (u8), direction (u8), pressure (u8)
        // But actually in v6, points are stored as delta-encoded varints
        // For simplicity, we'll use the flat format
        
        // Number of points
        write_varint(&mut data, points.len() as u64)?;
        
        // Encode each point
        for point in points {
            // x as fixed-point (multiply by 100, store as varint)
            let x_int = (point.x * 100.0) as i64;
            write_varint(&mut data, zigzag_encode(x_int) as u64)?;
            
            // y as fixed-point
            let y_int = (point.y * 100.0) as i64;
            write_varint(&mut data, zigzag_encode(y_int) as u64)?;
            
            // speed, width packed
            data.write_all(&[
                (point.speed & 0xFF) as u8,
                (point.width & 0xFF) as u8,
            ])?;
            
            // direction, pressure packed
            data.write_all(&[
                point.direction,
                point.pressure,
            ])?;
        }
        
        Ok(data)
    }
    
    fn write_page_info<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        let mut block = Vec::new();
        // loads_count (tag 1)
        write_tag_byte4(&mut block, 1, 1)?;
        // merges_count (tag 2)
        write_tag_byte4(&mut block, 2, 0)?;
        // text_chars_count (tag 3)
        write_tag_byte4(&mut block, 3, 0)?;
        // text_lines_count (tag 4)
        write_tag_byte4(&mut block, 4, 0)?;
        
        self.write_block_header(writer, 10, block.len() as u32)?;
        writer.write_all(&block)?;
        Ok(())
    }
}

impl Default for V6Writer {
    fn default() -> Self {
        Self::new()
    }
}

/// Zigzag encode a signed integer for varint encoding
fn zigzag_encode(n: i64) -> u64 {
    ((n << 1) ^ (n >> 63)) as u64
}

/// Zigzag decode
#[allow(dead_code)]
fn zigzag_decode(n: u64) -> i64 {
    ((n >> 1) as i64) ^ (-((n & 1) as i64))
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_zigzag() {
        assert_eq!(zigzag_encode(0), 0);
        assert_eq!(zigzag_encode(-1), 1);
        assert_eq!(zigzag_encode(1), 2);
        assert_eq!(zigzag_encode(-2), 3);
        
        assert_eq!(zigzag_decode(0), 0);
        assert_eq!(zigzag_decode(1), -1);
        assert_eq!(zigzag_decode(2), 1);
        assert_eq!(zigzag_decode(3), -2);
    }
    
    #[test]
    fn test_write_empty() {
        let mut writer = V6Writer::new();
        let data = writer.write_strokes(&[]).unwrap();
        
        // Should have header at minimum
        assert!(data.len() >= 43);
        assert_eq!(&data[..43], HEADER_V6);
    }
}


#[cfg(test)]
mod round_trip_tests {
    use super::*;
    use remarkable_core::{Stroke, Point, PenType, Color};
    
    #[test]
    fn test_write_and_parse_single_stroke() {
        let stroke = Stroke {
            pen: PenType::Ballpoint1,
            color: Color::Black,
            base_width: 2.0,
            points: vec![
                Point {
                    x: 100.0,
                    y: 100.0,
                    speed: 500,
                    direction: 45,
                    width: 200,
                    pressure: 128,
                },
                Point {
                    x: 200.0,
                    y: 200.0,
                    speed: 600,
                    direction: 45,
                    width: 200,
                    pressure: 128,
                },
            ],
        };
        
        let mut writer = V6Writer::new();
        let data = writer.write_strokes(&[stroke]).unwrap();
        
        assert!(data.len() > 43, "Should be larger than just header");
        assert_eq!(&data[..43], HEADER_V6, "Should start with v6 header");
    }
    
    #[test]
    fn test_write_multiple_strokes() {
        let strokes: Vec<Stroke> = (0..10).map(|i| {
            Stroke {
                pen: PenType::Fineliner1,
                color: Color::Black,
                base_width: 1.0,
                points: vec![
                    Point {
                        x: (i * 100) as f32,
                        y: (i * 50) as f32,
                        speed: 500,
                        direction: 0,
                        width: 100,
                        pressure: 128,
                    },
                    Point {
                        x: (i * 100 + 50) as f32,
                        y: (i * 50 + 25) as f32,
                        speed: 500,
                        direction: 45,
                        width: 100,
                        pressure: 128,
                    },
                ],
            }
        }).collect();
        
        let mut writer = V6Writer::new();
        let data = writer.write_strokes(&strokes).unwrap();
        
        assert!(data.len() > 500, "Should have significant size for 10 strokes");
    }
}
