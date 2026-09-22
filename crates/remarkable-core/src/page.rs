//! Page types and templates

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::stroke::Stroke;

/// Known page templates
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageTemplate {
    Blank,
    BlankPortrait,
    BlankLandscape,
    Lines,
    LinesSmall,
    LinesMedium,
    Grid,
    GridSmall,
    GridMedium,
    Dots,
    USCollege,
    USLegal,
    Custom(String),
}

impl PageTemplate {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Blank => "Blank",
            Self::BlankPortrait => "P Blank",
            Self::BlankLandscape => "LS Blank",
            Self::Lines => "Lines",
            Self::LinesSmall => "Lines small",
            Self::LinesMedium => "Lines medium",
            Self::Grid => "Grid",
            Self::GridSmall => "Grid small",
            Self::GridMedium => "Grid medium",
            Self::Dots => "Dots",
            Self::USCollege => "P US College",
            Self::USLegal => "P US Legal",
            Self::Custom(s) => s,
        }
    }
    
    pub fn from_str(s: &str) -> Self {
        match s {
            "Blank" => Self::Blank,
            "P Blank" => Self::BlankPortrait,
            "LS Blank" => Self::BlankLandscape,
            "Lines" => Self::Lines,
            "Lines small" => Self::LinesSmall,
            "Lines medium" => Self::LinesMedium,
            "Grid" => Self::Grid,
            "Grid small" => Self::GridSmall,
            "Grid medium" => Self::GridMedium,
            "Dots" => Self::Dots,
            "P US College" => Self::USCollege,
            "P US Legal" => Self::USLegal,
            _ => Self::Custom(s.to_string()),
        }
    }
}

/// A single page in a document
#[derive(Debug, Clone)]
pub struct Page {
    /// Page UUID
    pub id: Uuid,
    /// Template name
    pub template: PageTemplate,
    /// Strokes on this page
    pub strokes: Vec<Stroke>,
    /// Vertical scroll position
    pub vertical_scroll: i32,
    /// Page index (for ordering)
    pub index: String,
}

impl Page {
    /// Create a new blank page
    pub fn new(template: PageTemplate) -> Self {
        Self {
            id: Uuid::new_v4(),
            template,
            strokes: vec![],
            vertical_scroll: 0,
            index: String::new(),
        }
    }
}
