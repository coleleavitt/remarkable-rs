//! Example: Parse and render .rm files
//!
//! Usage:
//!   cargo run -p remarkable-lines --example parse_rm -- <file.rm> [output.svg]

use remarkable_lines::{parse_rm_file, detect_version, strokes_to_svg};
use std::env;
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    
    if args.len() < 2 {
        eprintln!("Usage: {} <file.rm> [output.svg]", args[0]);
        std::process::exit(1);
    }
    
    let input = &args[1];
    let output = args.get(2).map(|s| s.as_str());
    
    println!("Parsing: {}", input);
    
    let data = fs::read(input)?;
    
    // Detect version
    let version = detect_version(&data).unwrap_or(0);
    println!("Detected version: {}", version);
    
    // Parse strokes
    let strokes = parse_rm_file(&data)?;
    
    println!("Strokes: {}", strokes.len());
    
    // Print stroke stats
    let total_points: usize = strokes.iter().map(|s| s.points.len()).sum();
    println!("Total points: {}", total_points);
    
    // Show first 10 strokes
    for (i, stroke) in strokes.iter().take(10).enumerate() {
        println!("  Stroke {}: {:?} {:?} {} points", 
            i, stroke.pen, stroke.color, stroke.points.len());
    }
    
    if strokes.len() > 10 {
        println!("  ... and {} more strokes", strokes.len() - 10);
    }
    
    // Export to SVG if requested
    if let Some(svg_path) = output {
        println!("\nExporting to: {}", svg_path);
        let svg = strokes_to_svg(&strokes, 1872, 1404);
        fs::write(svg_path, svg)?;
        println!("Done!");
    }
    
    Ok(())
}
