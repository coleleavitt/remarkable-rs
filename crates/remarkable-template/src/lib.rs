//! Template parser and writer for reMarkable tablet
//!
//! Templates are SVG or PNG files with JSON metadata.
//! Located in /usr/share/remarkable/templates/ on device.

use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TemplateError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("PNG error: {0}")]
    Png(#[from] png::DecodingError),
    #[error("Invalid template format")]
    InvalidFormat,
}

/// Template category
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TemplateCategory {
    Blank,
    Lines,
    Grids,
    Music,
    Planners,
    Custom,
}

/// Template orientation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Orientation {
    Portrait,
    Landscape,
}

/// Template metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateMetadata {
    pub name: String,
    pub filename: String,
    #[serde(rename = "iconCode")]
    pub icon_code: String,
    pub categories: Vec<String>,
    #[serde(default)]
    pub landscape: bool,
}

/// Template file (SVG or PNG)
#[derive(Debug, Clone)]
pub enum TemplateFormat {
    Svg(String),
    Png(Vec<u8>),
}

/// A reMarkable template
#[derive(Debug, Clone)]
pub struct Template {
    pub metadata: TemplateMetadata,
    pub format: TemplateFormat,
}

impl Template {
    /// Create a new template
    pub fn new(name: &str, format: TemplateFormat, categories: Vec<String>) -> Self {
        let filename = name.to_lowercase().replace(' ', "_");
        Self {
            metadata: TemplateMetadata {
                name: name.to_string(),
                filename: filename.clone(),
                icon_code: "\u{e9fe}".to_string(),
                categories,
                landscape: false,
            },
            format,
        }
    }
    
    /// Load template from file
    pub fn load(path: &Path) -> Result<Self, TemplateError> {
        let ext = path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        
        let format = match ext.to_lowercase().as_str() {
            "svg" => {
                let content = std::fs::read_to_string(path)?;
                TemplateFormat::Svg(content)
            }
            "png" => {
                let data = std::fs::read(path)?;
                TemplateFormat::Png(data)
            }
            _ => return Err(TemplateError::InvalidFormat),
        };
        
        let name = path.file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown")
            .to_string();
        
        Ok(Self {
            metadata: TemplateMetadata {
                name: name.clone(),
                filename: name,
                icon_code: "\u{e9fe}".to_string(),
                categories: vec!["Custom".to_string()],
                landscape: false,
            },
            format,
        })
    }
    
    /// Save template to file
    pub fn save(&self, dir: &Path) -> Result<(), TemplateError> {
        let ext = match &self.format {
            TemplateFormat::Svg(_) => "svg",
            TemplateFormat::Png(_) => "png",
        };
        
        let path = dir.join(format!("{}.{}", self.metadata.filename, ext));
        
        match &self.format {
            TemplateFormat::Svg(content) => {
                std::fs::write(path, content)?;
            }
            TemplateFormat::Png(data) => {
                std::fs::write(path, data)?;
            }
        }
        
        Ok(())
    }
    
    /// Create a blank SVG template
    pub fn blank(name: &str, width: u32, height: u32) -> Self {
        let svg = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 {} {}">
  <rect width="100%" height="100%" fill="white"/>
</svg>"#,
            width, height, width, height
        );
        
        Self::new(name, TemplateFormat::Svg(svg), vec!["Blank".to_string()])
    }
    
    /// Create a lined template
    pub fn lined(name: &str, width: u32, height: u32, line_spacing: u32) -> Self {
        let mut lines = String::new();
        let mut y = line_spacing;
        while y < height {
            lines.push_str(&format!(
                "  <line x1=\"0\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#e0e0e0\" stroke-width=\"1\"/>\n",
                y, width, y
            ));
            y += line_spacing;
        }
        
        let svg = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 {} {}">
  <rect width="100%" height="100%" fill="white"/>
{}
</svg>"#,
            width, height, width, height, lines
        );
        
        Self::new(name, TemplateFormat::Svg(svg), vec!["Lines".to_string()])
    }
    
    /// Create a grid template
    pub fn grid(name: &str, width: u32, height: u32, cell_size: u32) -> Self {
        let mut lines = String::new();
        
        // Vertical lines
        let mut x = cell_size;
        while x < width {
            lines.push_str(&format!(
                "  <line x1=\"{}\" y1=\"0\" x2=\"{}\" y2=\"{}\" stroke=\"#e0e0e0\" stroke-width=\"1\"/>\n",
                x, x, height
            ));
            x += cell_size;
        }
        
        // Horizontal lines
        let mut y = cell_size;
        while y < height {
            lines.push_str(&format!(
                "  <line x1=\"0\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#e0e0e0\" stroke-width=\"1\"/>\n",
                y, width, y
            ));
            y += cell_size;
        }
        
        let svg = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 {} {}">
  <rect width="100%" height="100%" fill="white"/>
{}
</svg>"#,
            width, height, width, height, lines
        );
        
        Self::new(name, TemplateFormat::Svg(svg), vec!["Grids".to_string()])
    }
}

/// Parse templates.json from device
pub fn parse_templates_json(json: &str) -> Result<Vec<TemplateMetadata>, TemplateError> {
    #[derive(Deserialize)]
    struct TemplatesFile {
        templates: Vec<TemplateMetadata>,
    }
    
    let file: TemplatesFile = serde_json::from_str(json)?;
    Ok(file.templates)
}

/// Generate templates.json content
pub fn generate_templates_json(templates: &[TemplateMetadata]) -> Result<String, TemplateError> {
    #[derive(Serialize)]
    struct TemplatesFile<'a> {
        templates: &'a [TemplateMetadata],
    }
    
    let file = TemplatesFile { templates };
    Ok(serde_json::to_string_pretty(&file)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_blank_template() {
        let t = Template::blank("Test", 1404, 1872);
        assert_eq!(t.metadata.name, "Test");
        assert!(matches!(t.format, TemplateFormat::Svg(_)));
    }
    
    #[test]
    fn test_lined_template() {
        let t = Template::lined("Lined", 1404, 1872, 40);
        if let TemplateFormat::Svg(svg) = &t.format {
            assert!(svg.contains("<line"));
        }
    }
    
    #[test]
    fn test_grid_template() {
        let t = Template::grid("Grid", 1404, 1872, 50);
        if let TemplateFormat::Svg(svg) = &t.format {
            assert!(svg.contains("<line"));
        }
    }
}
