//! E-ink display waveform format traits
//!
//! reMarkable devices use e-ink displays that require specific
//! waveforms for updating. This module provides:
//!
//! - WBF format (RM1/RM2)
//! - ACEP2/Gallery format (Paper Pro)
//! - Display mode definitions
//! - Temperature compensation

/// Display update mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DisplayMode {
    /// Full refresh (GC16) - best quality, slowest
    Full,
    /// Partial refresh (DU) - fast, ghosting
    Partial,
    /// Fast grayscale (A2) - very fast, 1-bit
    Fast,
    /// High quality gray (GL16)
    GrayQuality,
    /// Animation mode (DU4)
    Animation,
    /// Reagl (reduced ghosting)
    Reagl,
    /// Reagl-D (reduced ghosting, dithered)
    ReaglD,
    /// Init mode (clear to white)
    Init,
    /// Unknown mode
    Unknown(u8),
}

impl DisplayMode {
    /// Create from mode ID
    pub fn from_id(id: u8) -> Self {
        match id {
            0 => Self::Init,
            1 => Self::Partial,
            2 => Self::Full,
            3 => Self::GrayQuality,
            4 => Self::Fast,
            5 => Self::Animation,
            6 => Self::Reagl,
            7 => Self::ReaglD,
            _ => Self::Unknown(id),
        }
    }
    
    /// Get mode ID
    pub fn id(&self) -> u8 {
        match self {
            Self::Init => 0,
            Self::Partial => 1,
            Self::Full => 2,
            Self::GrayQuality => 3,
            Self::Fast => 4,
            Self::Animation => 5,
            Self::Reagl => 6,
            Self::ReaglD => 7,
            Self::Unknown(id) => *id,
        }
    }
    
    /// Human-readable name
    pub fn name(&self) -> &'static str {
        match self {
            Self::Full => "Full (GC16)",
            Self::Partial => "Partial (DU)",
            Self::Fast => "Fast (A2)",
            Self::GrayQuality => "Gray Quality (GL16)",
            Self::Animation => "Animation (DU4)",
            Self::Reagl => "REAGL",
            Self::ReaglD => "REAGL-D",
            Self::Init => "Init",
            Self::Unknown(_) => "Unknown",
        }
    }
    
    /// Expected refresh time in ms
    pub fn refresh_time_ms(&self) -> u32 {
        match self {
            Self::Full => 450,
            Self::Partial => 260,
            Self::Fast => 120,
            Self::GrayQuality => 400,
            Self::Animation => 100,
            Self::Reagl => 300,
            Self::ReaglD => 350,
            Self::Init => 1000,
            Self::Unknown(_) => 500,
        }
    }
}

/// Temperature range for waveform selection
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TempRange {
    /// Minimum temperature in Celsius
    pub min: i8,
    /// Maximum temperature in Celsius
    pub max: i8,
}

impl TempRange {
    pub fn new(min: i8, max: i8) -> Self {
        Self { min, max }
    }
    
    /// Check if a temperature falls in this range
    pub fn contains(&self, temp: i8) -> bool {
        temp >= self.min && temp <= self.max
    }
    
    /// Standard room temperature range
    pub fn room_temp() -> Self {
        Self::new(20, 28)
    }
}

/// Panel information
#[derive(Debug, Clone)]
pub struct PanelInfo {
    /// Panel identifier
    pub id: String,
    /// Panel width in pixels
    pub width: u32,
    /// Panel height in pixels
    pub height: u32,
    /// Bits per pixel
    pub bpp: u8,
    /// Panel type description
    pub panel_type: String,
}

impl PanelInfo {
    /// Standard RM1 panel
    pub fn rm1() -> Self {
        Self {
            id: "RM1".to_string(),
            width: 1872,
            height: 1404,
            bpp: 4,
            panel_type: "ED060XH2".to_string(),
        }
    }
    
    /// Standard RM2 panel  
    pub fn rm2() -> Self {
        Self {
            id: "RM2".to_string(),
            width: 1872,
            height: 1404,
            bpp: 4,
            panel_type: "ED060XH8".to_string(),
        }
    }
    
    /// Paper Pro panel
    pub fn paper_pro() -> Self {
        Self {
            id: "PaperPro".to_string(),
            width: 2160,
            height: 1620,
            bpp: 8,
            panel_type: "ACEP2".to_string(),
        }
    }
}

/// Waveform error type
#[derive(Debug, thiserror::Error)]
pub enum WaveformError {
    #[error("invalid header")]
    InvalidHeader,
    
    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),
    
    #[error("invalid data: {0}")]
    InvalidData(String),
    
    #[error("mode not found: {0}")]
    ModeNotFound(String),
    
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Core waveform data trait
///
/// Implementations parse and provide access to waveform data
/// for different panel types.
pub trait WaveformData: Send + Sync {
    /// Parse waveform from raw data
    fn parse(data: &[u8]) -> Result<Self, WaveformError> where Self: Sized;
    
    /// Get supported display modes
    fn modes(&self) -> &[DisplayMode];
    
    /// Get temperature ranges
    fn temperature_ranges(&self) -> &[TempRange];
    
    /// Get panel information
    fn panel_info(&self) -> &PanelInfo;
    
    /// Get waveform data for a specific mode and temperature
    fn get_waveform(&self, mode: DisplayMode, temp: i8) -> Option<&[u8]>;
    
    /// Get the default mode for general use
    fn default_mode(&self) -> DisplayMode {
        DisplayMode::Full
    }
    
    /// Suggest mode for pen input (fast, partial updates)
    fn pen_mode(&self) -> DisplayMode {
        DisplayMode::Fast
    }
}

/// WBF format (RM1/RM2)
#[derive(Debug, Clone)]
pub struct WbfWaveform {
    /// Supported modes
    modes: Vec<DisplayMode>,
    /// Temperature ranges
    temps: Vec<TempRange>,
    /// Panel info
    panel: PanelInfo,
    /// Waveform data (placeholder)
    _data: Vec<u8>,
}

impl WbfWaveform {
    /// Create empty WBF structure
    pub fn empty(panel: PanelInfo) -> Self {
        Self {
            modes: vec![DisplayMode::Full, DisplayMode::Partial, DisplayMode::Fast],
            temps: vec![TempRange::room_temp()],
            panel,
            _data: Vec::new(),
        }
    }
}

impl WaveformData for WbfWaveform {
    fn parse(_data: &[u8]) -> Result<Self, WaveformError> {
        // Placeholder - actual parsing would validate header and extract modes
        Ok(Self::empty(PanelInfo::rm2()))
    }
    
    fn modes(&self) -> &[DisplayMode] {
        &self.modes
    }
    
    fn temperature_ranges(&self) -> &[TempRange] {
        &self.temps
    }
    
    fn panel_info(&self) -> &PanelInfo {
        &self.panel
    }
    
    fn get_waveform(&self, _mode: DisplayMode, _temp: i8) -> Option<&[u8]> {
        // Placeholder
        None
    }
}

/// ACEP2/Gallery format (Paper Pro)
#[derive(Debug, Clone)]
pub struct Acep2Waveform {
    /// Supported modes
    modes: Vec<DisplayMode>,
    /// Temperature ranges
    temps: Vec<TempRange>,
    /// Panel info
    panel: PanelInfo,
}

impl Acep2Waveform {
    /// Create empty ACEP2 structure
    pub fn empty() -> Self {
        Self {
            modes: vec![
                DisplayMode::Full,
                DisplayMode::Partial,
                DisplayMode::Fast,
                DisplayMode::GrayQuality,
            ],
            temps: vec![TempRange::room_temp()],
            panel: PanelInfo::paper_pro(),
        }
    }
}

impl WaveformData for Acep2Waveform {
    fn parse(_data: &[u8]) -> Result<Self, WaveformError> {
        // Placeholder
        Ok(Self::empty())
    }
    
    fn modes(&self) -> &[DisplayMode] {
        &self.modes
    }
    
    fn temperature_ranges(&self) -> &[TempRange] {
        &self.temps
    }
    
    fn panel_info(&self) -> &PanelInfo {
        &self.panel
    }
    
    fn get_waveform(&self, _mode: DisplayMode, _temp: i8) -> Option<&[u8]> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_display_mode_roundtrip() {
        for id in 0..8 {
            let mode = DisplayMode::from_id(id);
            assert_eq!(mode.id(), id);
        }
    }
    
    #[test]
    fn test_temp_range() {
        let range = TempRange::room_temp();
        assert!(range.contains(25));
        assert!(!range.contains(35));
    }
    
    #[test]
    fn test_panel_info() {
        let rm2 = PanelInfo::rm2();
        assert_eq!(rm2.width, 1872);
        assert_eq!(rm2.height, 1404);
        
        let pp = PanelInfo::paper_pro();
        assert_eq!(pp.width, 2160);
    }
}
