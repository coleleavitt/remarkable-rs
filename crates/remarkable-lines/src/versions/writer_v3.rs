//! Version 3 .rm writer - Flat stroke format
//!
//! Creates .rm files compatible with firmware 1.x through 2.5.

use std::io::{Cursor, Write};
use byteorder::{LittleEndian, WriteBytesExt};
use remarkable_core::Stroke;
use crate::LinesError;
use crate::versions::HEADER_V3;

/// Writer for .rm format version 3
pub struct V3Writer;

impl V3Writer {
    /// Write strokes to v3 .rm format
    pub fn write(strokes: &[Stroke]) -> Result<Vec<u8>, LinesError> {
        let mut buffer = Cursor::new(Vec::new());
        
        // Write header
        buffer.write_all(HEADER_V3)
            .map_err(|e| LinesError::InvalidData(e.to_string()))?;
        
        // Write number of layers (always 1 for v3)
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
    use crate::versions::v3::V3Parser;
    use crate::versions::RmParser;
    
    #[test]
    fn test_v3_write_empty() {
        let data = V3Writer::write(&[]).unwrap();
        let mut parser = V3Parser::new(data).unwrap();
        let strokes = parser.parse_strokes().unwrap();
        assert!(strokes.is_empty());
    }
    
    #[test]
    fn test_v3_round_trip() {
        let strokes = vec![
            Stroke {
                pen: PenType::Ballpoint1,
                color: Color::Black,
                base_width: 2.0,
                points: vec![
                    Point { x: 100.0, y: 200.0, speed: 500, width: 200, direction: 128, pressure: 200 },
                    Point { x: 150.0, y: 250.0, speed: 600, width: 210, direction: 64, pressure: 180 },
                ],
            },
        ];
        
        let data = V3Writer::write(&strokes).unwrap();
        let mut parser = V3Parser::new(data).unwrap();
        let parsed = parser.parse_strokes().unwrap();
        
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].points.len(), 2);
        // Check coordinates match (floats will be exact)
        assert!((parsed[0].points[0].x - 100.0).abs() < 0.01);
        assert!((parsed[0].points[0].y - 200.0).abs() < 0.01);
    }
}
