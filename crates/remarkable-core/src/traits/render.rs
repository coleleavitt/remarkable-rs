//! Stroke rendering traits for multiple backends
//!
//! This module provides a unified rendering interface that can
//! target different output backends: SVG, raster, PDF, etc.

use crate::{Layer, Page, Stroke};
use super::color::Rgba;
use super::pen::RenderParams;

/// Rendering context with shared state
#[derive(Debug, Clone)]
pub struct RenderContext {
    /// Canvas width
    pub width: u32,
    /// Canvas height
    pub height: u32,
    /// Current transform matrix (3x3, row-major)
    pub transform: [f32; 9],
    /// Current clip region (x, y, w, h)
    pub clip: Option<(f32, f32, f32, f32)>,
    /// Global opacity multiplier (0.0 - 1.0)
    pub opacity: f32,
}

impl RenderContext {
    /// Create with dimensions
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            transform: [
                1.0, 0.0, 0.0,
                0.0, 1.0, 0.0,
                0.0, 0.0, 1.0,
            ],
            clip: None,
            opacity: 1.0,
        }
    }
    
    /// Create for standard reMarkable portrait page
    pub fn remarkable_portrait() -> Self {
        Self::new(1404, 1872)
    }
    
    /// Create for standard reMarkable landscape page
    pub fn remarkable_landscape() -> Self {
        Self::new(1872, 1404)
    }
    
    /// Apply translation
    pub fn translate(&mut self, dx: f32, dy: f32) {
        self.transform[2] += dx;
        self.transform[5] += dy;
    }
    
    /// Apply uniform scale
    pub fn scale(&mut self, s: f32) {
        self.transform[0] *= s;
        self.transform[4] *= s;
    }
    
    /// Reset transform to identity
    pub fn reset_transform(&mut self) {
        self.transform = [
            1.0, 0.0, 0.0,
            0.0, 1.0, 0.0,
            0.0, 0.0, 1.0,
        ];
    }
    
    /// Set clip region
    pub fn set_clip(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.clip = Some((x, y, w, h));
    }
    
    /// Clear clip region
    pub fn clear_clip(&mut self) {
        self.clip = None;
    }
}

/// Canvas trait for renderers to draw into
///
/// Implementations provide backend-specific drawing operations.
pub trait Canvas: Send + Sync {
    /// Get canvas dimensions
    fn dimensions(&self) -> (u32, u32);
    
    /// Clear the canvas with a color
    fn clear(&mut self, color: Rgba);
    
    /// Begin a new path
    fn begin_path(&mut self);
    
    /// Move to point
    fn move_to(&mut self, x: f32, y: f32);
    
    /// Line to point
    fn line_to(&mut self, x: f32, y: f32);
    
    /// Quadratic curve to point
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32);
    
    /// Cubic curve to point
    fn cubic_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32);
    
    /// Close the current path
    fn close_path(&mut self);
    
    /// Stroke the current path
    fn stroke(&mut self, color: Rgba, width: f32);
    
    /// Fill the current path
    fn fill(&mut self, color: Rgba);
    
    /// Save current state
    fn save(&mut self);
    
    /// Restore previous state
    fn restore(&mut self);
    
    /// Apply transform
    fn set_transform(&mut self, transform: &[f32; 9]);
    
    /// Set global opacity
    fn set_opacity(&mut self, opacity: f32);
}

/// Rendering error type
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("canvas not initialized")]
    NotInitialized,
    
    #[error("invalid stroke: {0}")]
    InvalidStroke(String),
    
    #[error("render failed: {0}")]
    RenderFailed(String),
}

/// Core stroke renderer trait
///
/// Implementations render strokes to a specific canvas type.
pub trait StrokeRenderer: Send + Sync {
    /// Canvas type for this renderer
    type Canvas: Canvas;
    
    /// Render a single stroke
    fn render_stroke(&self, stroke: &Stroke, canvas: &mut Self::Canvas, ctx: &RenderContext) -> Result<(), RenderError>;
    
    /// Render a layer (all strokes)
    fn render_layer(&self, layer: &Layer, canvas: &mut Self::Canvas, ctx: &RenderContext) -> Result<(), RenderError> {
        for stroke in &layer.strokes {
            self.render_stroke(stroke, canvas, ctx)?;
        }
        Ok(())
    }
    
    /// Render a full page
    fn render_page(&self, page: &Page, canvas: &mut Self::Canvas, ctx: &RenderContext) -> Result<(), RenderError> {
        for layer in &page.layers {
            if layer.visible {
                self.render_layer(layer, canvas, ctx)?;
            }
        }
        Ok(())
    }
    
    /// Get render params for a stroke based on pen type
    fn params_for_stroke(&self, stroke: &Stroke) -> RenderParams;
}

/// SVG canvas implementation
pub struct SvgCanvas {
    /// SVG elements buffer
    pub elements: Vec<String>,
    /// Width
    pub width: u32,
    /// Height
    pub height: u32,
    /// Current path data
    current_path: String,
}

impl SvgCanvas {
    /// Create a new SVG canvas
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            elements: Vec::new(),
            width,
            height,
            current_path: String::new(),
        }
    }
    
    /// Generate complete SVG string
    pub fn to_svg(&self) -> String {
        let mut svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 {} {}">"#,
            self.width, self.height, self.width, self.height
        );
        svg.push('\n');
        for elem in &self.elements {
            svg.push_str(elem);
            svg.push('\n');
        }
        svg.push_str("</svg>");
        svg
    }
}

impl Canvas for SvgCanvas {
    fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
    
    fn clear(&mut self, color: Rgba) {
        self.elements.push(format!(
            r#"<rect width="100%" height="100%" fill="{}"/>"#,
            color.to_hex()
        ));
    }
    
    fn begin_path(&mut self) {
        self.current_path.clear();
    }
    
    fn move_to(&mut self, x: f32, y: f32) {
        self.current_path.push_str(&format!("M {} {} ", x, y));
    }
    
    fn line_to(&mut self, x: f32, y: f32) {
        self.current_path.push_str(&format!("L {} {} ", x, y));
    }
    
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        self.current_path.push_str(&format!("Q {} {} {} {} ", cx, cy, x, y));
    }
    
    fn cubic_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        self.current_path.push_str(&format!("C {} {} {} {} {} {} ", c1x, c1y, c2x, c2y, x, y));
    }
    
    fn close_path(&mut self) {
        self.current_path.push_str("Z ");
    }
    
    fn stroke(&mut self, color: Rgba, width: f32) {
        self.elements.push(format!(
            r#"<path d="{}" stroke="{}" stroke-width="{:.2}" fill="none" stroke-linecap="round" stroke-linejoin="round"/>"#,
            self.current_path.trim(),
            color.to_hex(),
            width
        ));
        self.current_path.clear();
    }
    
    fn fill(&mut self, color: Rgba) {
        self.elements.push(format!(
            r#"<path d="{}" fill="{}"/>"#,
            self.current_path.trim(),
            color.to_hex()
        ));
        self.current_path.clear();
    }
    
    fn save(&mut self) {
        self.elements.push("<g>".to_string());
    }
    
    fn restore(&mut self) {
        self.elements.push("</g>".to_string());
    }
    
    fn set_transform(&mut self, transform: &[f32; 9]) {
        let t = transform;
        self.elements.push(format!(
            r#"<g transform="matrix({},{},{},{},{},{})">"#,
            t[0], t[3], t[1], t[4], t[2], t[5]
        ));
    }
    
    fn set_opacity(&mut self, opacity: f32) {
        self.elements.push(format!(r#"<g opacity="{:.2}">"#, opacity));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_render_context_transform() {
        let mut ctx = RenderContext::remarkable_portrait();
        ctx.translate(100.0, 50.0);
        
        assert_eq!(ctx.transform[2], 100.0);
        assert_eq!(ctx.transform[5], 50.0);
        
        ctx.reset_transform();
        assert_eq!(ctx.transform[2], 0.0);
    }
    
    #[test]
    fn test_svg_canvas_basic() {
        let mut canvas = SvgCanvas::new(100, 100);
        canvas.clear(Rgba::WHITE);
        canvas.begin_path();
        canvas.move_to(0.0, 0.0);
        canvas.line_to(100.0, 100.0);
        canvas.stroke(Rgba::BLACK, 2.0);
        
        let svg = canvas.to_svg();
        assert!(svg.contains("<svg"));
        assert!(svg.contains("M 0 0"));
        assert!(svg.contains("L 100 100"));
    }
}
