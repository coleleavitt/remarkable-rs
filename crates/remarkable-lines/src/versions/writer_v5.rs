//! Version 5 .rm writer - Layer-based format
//!
//! Creates .rm files compatible with firmware 2.6 through 2.x.

use std::io::{Cursor, Write};
use byteorder::{LittleEndian, WriteBytesExt};
use remarkable_core::Stroke;
use crate::LinesError;
use crate::versions::HEADER_V5;

/// Writer for .rm format version 5
pub struct V5Writer;

impl V5Writer {
    /// Write strokes to v5 .rm format
    pub fn write(strokes: &[Stroke]) -> Result<Vec<u8>, LinesError> {
        let mut buffer = Cursor::new(Vec::new());
        
        // Write header
        buffer.write_all(HEADER_V5)
            .map_err(|e| LinesError::InvalidData(e.to_string()))?;
        
        // Write number of layers (always 1 - single layer output)
        buffer.write_i32::<LittleEndian>(1)
            .map_err(|e| LinesError::InvalidData(e.to_string()))?;
        
        // Write number of strokes
        buffer.write_i32::<LittleEndian>(strokes.len() as i32)
            .map_err(|e| LinesError::InvalidData(e.to_string()))?;
        
        // Write each stroke
        for stroke in strokes {
            // Pen type
            buffer.write_i32::<LittleEndian>(stroke.pen.to_u32() as i32)
                .map_err(|e| LinesError::InvalidData(e.to_string()))?;
            
            // Color
            buffer.write_i32::<LittleEndian>(stroke.color.to_u32() as i32)
                .map_err(|e| LinesError::InvalidData(e.to_string()))?;
            
            // Unknown (0)
            buffer.write_i32::<LittleEndian>(0)
                .map_err(|e| LinesError::InvalidData(e.to_string()))?;
            
            // Base width
            buffer.write_f32::<LittleEndian>(stroke.base_width)
                .map_err(|e| LinesError::InvalidData(e.to_string()))?;
            
            // Unknown2 (v5 specific, 0)
            buffer.write_i32::<LittleEndian>(0)
                .map_err(|e| LinesError::InvalidData(e.to_string()))?;
            
            // Number of points
            buffer.write_i32::<LittleEndian>(stroke.points.len() as i32)
                .map_err(|e| LinesError::InvalidData(e.to_string()))?;
            
            // Write points
            for point in &stroke.points {
                buffer.write_f32::<LittleEndian>(point.x)
                    .map_err(|e| LinesError::InvalidData(e.to_string()))?;
                buffer.write_f32::<LittleEndian>(point.y)
                    .map_err(|e| LinesError::InvalidData(e.to_string()))?;
                
                // Convert back to f32 format
                let speed_f = point.speed as f32 / 10.0;
                let direction_f = point.direction as f32 * std::f32::consts::TAU / 255.0;
                let width_f = point.width as f32 / 100.0;
                let pressure_f = point.pressure as f32 / 255.0;
                
                buffer.write_f32::<LittleEndian>(speed_f)
                    .map_err(|e| LinesError::InvalidData(e.to_string()))?;
                buffer.write_f32::<LittleEndian>(direction_f)
                    .map_err(|e| LinesError::InvalidData(e.to_string()))?;
                buffer.write_f32::<LittleEndian>(width_f)
                    .map_err(|e| LinesError::InvalidData(e.to_string()))?;
                buffer.write_f32::<LittleEndian>(pressure_f)
                    .map_err(|e| LinesError::InvalidData(e.to_string()))?;
            }
        }
        
        Ok(buffer.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use remarkable_core::{Point, PenType, Color};
    use crate::versions::v5::V5Parser;
    use crate::versions::RmParser;
    
    #[test]
    fn test_v5_write_empty() {
        let data = V5Writer::write(&[]).unwrap();
        let mut parser = V5Parser::new(data).unwrap();
        let strokes = parser.parse_strokes().unwrap();
        assert!(strokes.is_empty());
    }
    
    #[test]
    fn test_v5_round_trip() {
        let strokes = vec![
            Stroke {
                pen: PenType::Fineliner1,
                color: Color::Gray,
                base_width: 1.5,
                points: vec![
                    Point { x: 300.0, y: 400.0, speed: 800, width: 150, direction: 200, pressure: 230 },
                    Point { x: 350.0, y: 450.0, speed: 700, width: 160, direction: 180, pressure: 210 },
                    Point { x: 400.0, y: 500.0, speed: 600, width: 170, direction: 160, pressure: 190 },
                ],
            },
        ];
        
        let data = V5Writer::write(&strokes).unwrap();
        let mut parser = V5Parser::new(data).unwrap();
        let parsed = parser.parse_strokes().unwrap();
        
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].points.len(), 3);
        assert!((parsed[0].points[0].x - 300.0).abs() < 0.01);
        assert!((parsed[0].points[2].y - 500.0).abs() < 0.01);
    }
}
