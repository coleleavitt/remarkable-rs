//! Parser for reMarkable .rm/.lines files
//!
//! Supports all format versions:
//! - v3: Legacy flat format (firmware 1.x - 2.5)
//! - v5: Layered format (firmware 2.6 - 2.x)
//! - v6: CRDT-based tagged blocks (firmware 3.0+)
//!
//! # Example
//!
//! ```ignore
//! use remarkable_lines::{parse_rm_file, detect_version};
//!
//! let data = std::fs::read("document.rm")?;
//! let strokes = parse_rm_file(&data)?;
//! println!("Parsed {} strokes", strokes.len());
//! ```

pub mod versions;
mod parser_v6;
pub mod writer_v6;

pub use versions::{detect_version, create_parser, RmParser, HEADER_V3, HEADER_V5, HEADER_V6};
pub use versions::{V3Parser, V5Parser, V6Parser};
pub use versions::v6::{BlockType, BlockInfo};
pub use writer_v6::V6Writer;

use remarkable_core::Stroke;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum LinesError {
    #[error("Invalid or missing header")]
    InvalidHeader,
    
    #[error("Unsupported version: {0}")]
    UnsupportedVersion(u32),
    
    #[error("Unexpected end of file")]
    UnexpectedEof,
    
    #[error("Invalid data: {0}")]
    InvalidData(String),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Parse a .rm file and return all strokes
///
/// Automatically detects the format version and uses the appropriate parser.
pub fn parse_rm_file(data: &[u8]) -> Result<Vec<Stroke>, LinesError> {
    let mut parser = create_parser(data.to_vec())?;
    parser.parse_strokes()
}

/// Convert strokes to SVG string
pub fn strokes_to_svg(strokes: &[Stroke], width: u32, height: u32) -> String {
    let mut svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 {} {}">"#,
        width, height, width, height
    );
    svg.push_str("\n<rect width=\"100%\" height=\"100%\" fill=\"white\"/>\n");
    
    for stroke in strokes {
        if stroke.points.len() < 2 {
            continue;
        }
        
        let color = stroke.color.to_rgb();
        let width = (stroke.base_width * 2.0).max(0.5);
        
        // Build path data
        let mut path_data = String::new();
        for (i, point) in stroke.points.iter().enumerate() {
            if i == 0 {
                path_data.push_str(&format!("M {} {}", point.x, point.y));
            } else {
                path_data.push_str(&format!(" L {} {}", point.x, point.y));
            }
        }
        
        svg.push_str(&format!(
            r#"<path d="{}" stroke="{}" stroke-width="{:.2}" fill="none" stroke-linecap="round" stroke-linejoin="round"/>"#,
            path_data, color, width
        ));
        svg.push_str("\n");
    }
    
    svg.push_str("</svg>");
    svg
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_strokes_to_svg() {
        let strokes = vec![
            Stroke {
                pen: remarkable_core::PenType::Fineliner1,
                color: remarkable_core::Color::Black,
                base_width: 2.0,
                points: vec![
                    remarkable_core::Point { x: 0.0, y: 0.0, speed: 0, width: 100, direction: 0, pressure: 128 },
                    remarkable_core::Point { x: 100.0, y: 100.0, speed: 0, width: 100, direction: 0, pressure: 128 },
                ],
            },
        ];
        
        let svg = strokes_to_svg(&strokes, 1872, 1404);
        assert!(svg.contains("<svg"));
        assert!(svg.contains("M 0 0"));
        assert!(svg.contains("L 100 100"));
    }
}
