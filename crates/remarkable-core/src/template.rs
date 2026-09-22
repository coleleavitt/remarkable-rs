//! Template metadata types

use serde::{Deserialize, Serialize};

/// Template category
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TemplateCategory {
    /// Blank templates
    Blank,
    /// Grid patterns
    Grids,
    /// Line patterns
    Lines,
    /// Creative layouts
    Creative,
    /// Life/planning
    Life,
    /// Custom user templates
    Custom,
}

/// Template metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Template {
    /// Template name
    pub name: String,
    
    /// Template filename (without extension)
    pub filename: String,
    
    /// Icon code for display
    #[serde(rename = "iconCode")]
    pub icon_code: String,
    
    /// Category for organization
    #[serde(default)]
    pub categories: Vec<String>,
    
    /// Landscape orientation
    #[serde(default)]
    pub landscape: bool,
}

/// Template collection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateCollection {
    pub templates: Vec<Template>,
}

impl Default for TemplateCollection {
    fn default() -> Self {
        Self {
            templates: Vec::new(),
        }
    }
}

/// Built-in template names (from firmware analysis)
pub const BUILTIN_TEMPLATES: &[&str] = &[
    // Blank
    "Blank",
    
    // Grids
    "Dots S",
    "Dots S (landscape)",
    "Grid small",
    "Grid small (landscape)",
    "Grid margin large",
    "Grid margin large (landscape)",
    "Grid medium",
    "Grid medium (landscape)",
    "Isometric",
    "Isometric (landscape)",
    
    // Lines
    "Lines small",
    "Lines small (landscape)",
    "Lines medium",
    "Lines medium (landscape)",
    "Lines large",
    "Lines large (landscape)",
    "Margin small",
    "Margin medium",
    "Margin large",
    
    // Creative
    "Storyboard 1",
    "Storyboard 2",
    "Storyboard 3",
    "Storyboard 4",
    
    // Life
    "Checklist",
    "Checklist double",
    "Cornell notes",
    "Day planner",
    "Week planner US",
    "Week planner",
    "Weekly planner 4",
    "Monthly planner",
    "Music",
    "Perspective 1",
    "Perspective 2",
    "Piano sheets",
];
