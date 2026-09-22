//! Version 3 .rm parser - Flat stroke format
//!
//! This is the original format used in firmware 1.x through 2.5.
//! No layer support, flat list of strokes.
//!
//! File structure:
//!   - Header: "reMarkable .lines file, version=3          "
//!   - num_layers: i32 (always 1 for v3)
//!   - For each layer:
//!     - num_strokes: i32
//!     - For each stroke:
//!       - pen: i32
//!       - color: i32
//!       - unknown: i32
//!       - base_width: f32
//!       - num_points: i32
//!       - For each point:
//!         - x, y, speed, direction, width, pressure (6 x f32)

use std::io::Cursor;
use byteorder::{LittleEndian, ReadBytesExt};
use remarkable_core::{Stroke, Point, PenType, Color};
use crate::LinesError;
use crate::versions::{RmParser, HEADER_V3, HEADER_SIZE};

/// Parser for .rm format version 3
pub struct V3Parser {
    data: Vec<u8>,
}

impl V3Parser {
    pub fn new(data: Vec<u8>) -> Result<Self, LinesError> {
        if data.len() < HEADER_SIZE {
            return Err(LinesError::InvalidHeader);
        }
        if &data[..HEADER_SIZE] != HEADER_V3 {
            return Err(LinesError::InvalidHeader);
        }
        Ok(Self { data })
    }
}

impl RmParser for V3Parser {
    fn parse_strokes(&mut self) -> Result<Vec<Stroke>, LinesError> {
        let mut cursor = Cursor::new(&self.data[HEADER_SIZE..]);
        let mut strokes = Vec::new();
        
        // Read number of layers (always 1 for v3)
        let num_layers = cursor.read_i32::<LittleEndian>()
            .map_err(|_| LinesError::UnexpectedEof)?;
        
        for _ in 0..num_layers {
            let num_strokes = cursor.read_i32::<LittleEndian>()
                .map_err(|_| LinesError::UnexpectedEof)?;
            
            for _ in 0..num_strokes {
                let pen_id = cursor.read_i32::<LittleEndian>()
                    .map_err(|_| LinesError::UnexpectedEof)? as u32;
                let color_id = cursor.read_i32::<LittleEndian>()
                    .map_err(|_| LinesError::UnexpectedEof)? as u32;
                let _unknown = cursor.read_i32::<LittleEndian>()
                    .map_err(|_| LinesError::UnexpectedEof)?;
                let base_width = cursor.read_f32::<LittleEndian>()
                    .map_err(|_| LinesError::UnexpectedEof)?;
                let num_points = cursor.read_i32::<LittleEndian>()
                    .map_err(|_| LinesError::UnexpectedEof)? as usize;
                
                let mut points = Vec::with_capacity(num_points);
                for _ in 0..num_points {
                    let x = cursor.read_f32::<LittleEndian>()
                        .map_err(|_| LinesError::UnexpectedEof)?;
                    let y = cursor.read_f32::<LittleEndian>()
                        .map_err(|_| LinesError::UnexpectedEof)?;
                    let speed_f = cursor.read_f32::<LittleEndian>()
                        .map_err(|_| LinesError::UnexpectedEof)?;
                    let direction_f = cursor.read_f32::<LittleEndian>()
                        .map_err(|_| LinesError::UnexpectedEof)?;
                    let width_f = cursor.read_f32::<LittleEndian>()
                        .map_err(|_| LinesError::UnexpectedEof)?;
                    let pressure_f = cursor.read_f32::<LittleEndian>()
                        .map_err(|_| LinesError::UnexpectedEof)?;
                    
                    // Convert to compressed format
                    let speed = (speed_f * 10.0).clamp(0.0, 65535.0) as u16;
                    let width = (width_f * 100.0).clamp(0.0, 65535.0) as u16;
                    let direction = ((direction_f * 255.0 / std::f32::consts::TAU).abs() % 256.0) as u8;
                    let pressure = (pressure_f.clamp(0.0, 1.0) * 255.0) as u8;
                    
                    points.push(Point {
                        x,
                        y,
                        speed,
                        width,
                        direction,
                        pressure,
                    });
                }
                
                strokes.push(Stroke {
                    pen: PenType::from_u32(pen_id),
                    color: Color::from_u32(color_id),
                    base_width,
                    points,
                });
            }
        }
        
        Ok(strokes)
    }
    
    fn version(&self) -> u32 {
        3
    }
    
    fn point_size(&self) -> usize {
        24  // 6 x f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_v3_empty() {
        let mut data = HEADER_V3.to_vec();
        data.extend_from_slice(&0i32.to_le_bytes()); // 0 layers
        
        let mut parser = V3Parser::new(data).unwrap();
        let strokes = parser.parse_strokes().unwrap();
        assert!(strokes.is_empty());
    }
}
