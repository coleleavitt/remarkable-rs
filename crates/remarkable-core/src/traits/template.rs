//! Page template traits for custom backgrounds
//!
//! reMarkable supports various page templates (grids, lines, etc.)
//! This module provides traits for defining and rendering templates.

/// Template category
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TemplateCategory {
    /// Blank page
    Blank,
    /// Lined paper
    Lines,
    /// Grid patterns
    Grids,
    /// Music notation
    Music,
    /// Planning/calendar
    Planning,
    /// Perspective guides
    Perspective,
    /// Custom user templates
    Custom,
}

impl TemplateCategory {
    /// Get category name
    pub fn name(&self) -> &'static str {
        match self {
            Self::Blank => "Blank",
            Self::Lines => "Lines",
            Self::Grids => "Grids",
            Self::Music => "Music",
            Self::Planning => "Planning",
            Self::Perspective => "Perspective",
            Self::Custom => "Custom",
        }
    }
    
    /// Get icon code for category
    pub fn icon(&self) -> &'static str {
        match self {
            Self::Blank => "\u{e9a7}",
            Self::Lines => "\u{e9a8}",
            Self::Grids => "\u{e9a9}",
            Self::Music => "\u{e9aa}",
            Self::Planning => "\u{e9ab}",
            Self::Perspective => "\u{e9ac}",
            Self::Custom => "\u{e9ad}",
        }
    }
}

impl std::fmt::Display for TemplateCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Template orientation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Orientation {
    #[default]
    Portrait,
    Landscape,
}

impl Orientation {
    /// Get dimensions for reMarkable
    pub fn dimensions(&self) -> (u32, u32) {
        match self {
            Self::Portrait => (1404, 1872),
            Self::Landscape => (1872, 1404),
        }
    }
}

/// Template error type
#[derive(Debug, thiserror::Error)]
pub enum TemplateError {
    #[error("template not found: {0}")]
    NotFound(String),
    
    #[error("invalid dimensions")]
    InvalidDimensions,
    
    #[error("render failed: {0}")]
    RenderFailed(String),
    
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Core template definition trait
///
/// Implementations define the visual appearance and metadata
/// of a page template.
pub trait TemplateDefinition: Send + Sync {
    /// Template unique identifier
    fn id(&self) -> &str;
    
    /// Display name
    fn name(&self) -> &str;
    
    /// Categories this template belongs to
    fn categories(&self) -> &[TemplateCategory];
    
    /// Icon code for UI display
    fn icon_code(&self) -> &str;
    
    /// Whether this is a landscape template
    fn is_landscape(&self) -> bool {
        false
    }
    
    /// Get orientation
    fn orientation(&self) -> Orientation {
        if self.is_landscape() {
            Orientation::Landscape
        } else {
            Orientation::Portrait
        }
    }
    
    /// Get dimensions
    fn dimensions(&self) -> (u32, u32) {
        self.orientation().dimensions()
    }
}

/// Template renderer trait
///
/// Implementations produce visual output for templates.
pub trait TemplateRenderer: Send + Sync {
    /// Render template to PNG bytes
    fn render_png(&self, template: &dyn TemplateDefinition) -> Result<Vec<u8>, TemplateError>;
    
    /// Render template to SVG string
    fn render_svg(&self, template: &dyn TemplateDefinition) -> Result<String, TemplateError>;
    
    /// Render at specific dimensions
    fn render_sized(&self, template: &dyn TemplateDefinition, width: u32, height: u32) -> Result<Vec<u8>, TemplateError>;
}

/// Built-in template: Blank
#[derive(Debug, Clone)]
pub struct BlankTemplate {
    landscape: bool,
}

impl BlankTemplate {
    pub fn portrait() -> Self {
        Self { landscape: false }
    }
    
    pub fn landscape() -> Self {
        Self { landscape: true }
    }
}

impl TemplateDefinition for BlankTemplate {
    fn id(&self) -> &str {
        if self.landscape { "Blank-L" } else { "Blank" }
    }
    
    fn name(&self) -> &str {
        "Blank"
    }
    
    fn categories(&self) -> &[TemplateCategory] {
        &[TemplateCategory::Blank]
    }
    
    fn icon_code(&self) -> &str {
        "\u{e9a7}"
    }
    
    fn is_landscape(&self) -> bool {
        self.landscape
    }
}

/// Built-in template: Lined
#[derive(Debug, Clone)]
pub struct LinedTemplate {
    /// Line spacing in pixels
    pub spacing: u32,
    /// Whether to show margin line
    pub margin: bool,
    /// Landscape orientation
    pub landscape: bool,
}

impl Default for LinedTemplate {
    fn default() -> Self {
        Self {
            spacing: 50,
            margin: true,
            landscape: false,
        }
    }
}

impl LinedTemplate {
    pub fn with_spacing(mut self, spacing: u32) -> Self {
        self.spacing = spacing;
        self
    }
    
    pub fn without_margin(mut self) -> Self {
        self.margin = false;
        self
    }
    
    pub fn landscape(mut self) -> Self {
        self.landscape = true;
        self
    }
}

impl TemplateDefinition for LinedTemplate {
    fn id(&self) -> &str {
        "Lined"
    }
    
    fn name(&self) -> &str {
        "Lined"
    }
    
    fn categories(&self) -> &[TemplateCategory] {
        &[TemplateCategory::Lines]
    }
    
    fn icon_code(&self) -> &str {
        "\u{e9a8}"
    }
    
    fn is_landscape(&self) -> bool {
        self.landscape
    }
}

/// Built-in template: Grid
#[derive(Debug, Clone)]
pub struct GridTemplate {
    /// Grid cell size in pixels
    pub cell_size: u32,
    /// Whether to show small dots instead of lines
    pub dotted: bool,
    /// Landscape orientation
    pub landscape: bool,
}

impl Default for GridTemplate {
    fn default() -> Self {
        Self {
            cell_size: 40,
            dotted: false,
            landscape: false,
        }
    }
}

impl GridTemplate {
    pub fn small() -> Self {
        Self { cell_size: 20, ..Default::default() }
    }
    
    pub fn large() -> Self {
        Self { cell_size: 60, ..Default::default() }
    }
    
    pub fn dotted() -> Self {
        Self { dotted: true, ..Default::default() }
    }
}

impl TemplateDefinition for GridTemplate {
    fn id(&self) -> &str {
        if self.dotted { "Grid-Dotted" } else { "Grid" }
    }
    
    fn name(&self) -> &str {
        if self.dotted { "Dotted Grid" } else { "Grid" }
    }
    
    fn categories(&self) -> &[TemplateCategory] {
        &[TemplateCategory::Grids]
    }
    
    fn icon_code(&self) -> &str {
        "\u{e9a9}"
    }
    
    fn is_landscape(&self) -> bool {
        self.landscape
    }
}

/// Template registry for managing available templates
pub struct TemplateRegistry {
    templates: Vec<Box<dyn TemplateDefinition>>,
}

impl TemplateRegistry {
    /// Create empty registry
    pub fn new() -> Self {
        Self { templates: Vec::new() }
    }
    
    /// Create with built-in templates
    pub fn with_builtins() -> Self {
        let mut registry = Self::new();
        registry.register(Box::new(BlankTemplate::portrait()));
        registry.register(Box::new(BlankTemplate::landscape()));
        registry.register(Box::new(LinedTemplate::default()));
        registry.register(Box::new(GridTemplate::default()));
        registry.register(Box::new(GridTemplate::dotted()));
        registry
    }
    
    /// Register a template
    pub fn register(&mut self, template: Box<dyn TemplateDefinition>) {
        self.templates.push(template);
    }
    
    /// Get template by ID
    pub fn get(&self, id: &str) -> Option<&dyn TemplateDefinition> {
        self.templates.iter()
            .find(|t| t.id() == id)
            .map(|t| t.as_ref())
    }
    
    /// List templates in a category
    pub fn in_category(&self, category: TemplateCategory) -> Vec<&dyn TemplateDefinition> {
        self.templates.iter()
            .filter(|t| t.categories().contains(&category))
            .map(|t| t.as_ref())
            .collect()
    }
    
    /// List all templates
    pub fn all(&self) -> Vec<&dyn TemplateDefinition> {
        self.templates.iter().map(|t| t.as_ref()).collect()
    }
}

impl Default for TemplateRegistry {
    fn default() -> Self {
        Self::with_builtins()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_orientation_dimensions() {
        assert_eq!(Orientation::Portrait.dimensions(), (1404, 1872));
        assert_eq!(Orientation::Landscape.dimensions(), (1872, 1404));
    }
    
    #[test]
    fn test_template_registry() {
        let registry = TemplateRegistry::with_builtins();
        
        assert!(registry.get("Blank").is_some());
        assert!(registry.get("Lined").is_some());
        assert!(registry.get("NonExistent").is_none());
        
        let blanks = registry.in_category(TemplateCategory::Blank);
        assert!(!blanks.is_empty());
    }
    
    #[test]
    fn test_blank_template() {
        let blank = BlankTemplate::portrait();
        assert_eq!(blank.id(), "Blank");
        assert!(!blank.is_landscape());
        assert_eq!(blank.dimensions(), (1404, 1872));
        
        let landscape = BlankTemplate::landscape();
        assert!(landscape.is_landscape());
        assert_eq!(landscape.dimensions(), (1872, 1404));
    }
}
