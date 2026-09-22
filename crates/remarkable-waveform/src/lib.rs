//! E-ink waveform parser for reMarkable displays
//!
//! Parses WBF (Waveform Binary Format) for RM1/RM2 and
//! ACEP2 .eink format for Paper Pro color displays.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum WaveformError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid waveform format")]
    InvalidFormat,
    #[error("Unsupported version: {0}")]
    UnsupportedVersion(u32),
}

/// Waveform mode for RM1/RM2
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaveformMode {
    /// Init - clear to white
    Init = 0,
    /// Direct update - fast, low quality
    Du = 1,
    /// Gray 16 - 16 grayscale levels
    Gc16 = 2,
    /// Gray 16 fast
    Gc16Fast = 3,
    /// Animation mode
    A2 = 4,
    /// Gray 16 local
    Gl16 = 5,
    /// Gray 16 local fast
    Gl16Fast = 6,
    /// Direct update 4
    Du4 = 7,
    /// Reagl
    Reagl = 8,
    /// Reagld
    Reagld = 9,
    /// Gray 16 local highlight
    Gl16Inv = 10,
}

impl WaveformMode {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Init),
            1 => Some(Self::Du),
            2 => Some(Self::Gc16),
            3 => Some(Self::Gc16Fast),
            4 => Some(Self::A2),
            5 => Some(Self::Gl16),
            6 => Some(Self::Gl16Fast),
            7 => Some(Self::Du4),
            8 => Some(Self::Reagl),
            9 => Some(Self::Reagld),
            10 => Some(Self::Gl16Inv),
            _ => None,
        }
    }
    
    pub fn name(&self) -> &'static str {
        match self {
            Self::Init => "INIT",
            Self::Du => "DU",
            Self::Gc16 => "GC16",
            Self::Gc16Fast => "GC16_FAST",
            Self::A2 => "A2",
            Self::Gl16 => "GL16",
            Self::Gl16Fast => "GL16_FAST",
            Self::Du4 => "DU4",
            Self::Reagl => "REAGL",
            Self::Reagld => "REAGLD",
            Self::Gl16Inv => "GL16_INV",
        }
    }
    
    /// Recommended use case
    pub fn description(&self) -> &'static str {
        match self {
            Self::Init => "Initialize display to white",
            Self::Du => "Fast monochrome update",
            Self::Gc16 => "Full 16-level grayscale",
            Self::Gc16Fast => "Fast grayscale update",
            Self::A2 => "Animation/drawing mode",
            Self::Gl16 => "Local grayscale update",
            Self::Gl16Fast => "Fast local grayscale",
            Self::Du4 => "4-level direct update",
            Self::Reagl => "Reduced ghosting algorithm",
            Self::Reagld => "Reduced ghosting dithered",
            Self::Gl16Inv => "Inverted local update",
        }
    }
}

/// Temperature range for waveform
#[derive(Debug, Clone)]
pub struct TemperatureRange {
    pub min: i8,
    pub max: i8,
}

/// WBF file header
#[derive(Debug, Clone)]
pub struct WbfHeader {
    pub version: u32,
    pub width: u32,
    pub height: u32,
    pub panel_id: u32,
    pub num_modes: u32,
    pub num_temps: u32,
}

/// A single waveform
#[derive(Debug, Clone)]
pub struct Waveform {
    pub mode: WaveformMode,
    pub temp_range: TemperatureRange,
    pub frame_count: u32,
    pub data: Vec<u8>,
}

/// WBF (Waveform Binary Format) file
#[derive(Debug)]
pub struct WbfFile {
    pub header: WbfHeader,
    pub waveforms: Vec<Waveform>,
}

impl WbfFile {
    /// Parse WBF from file
    pub fn parse(path: &Path) -> Result<Self, WaveformError> {
        let data = std::fs::read(path)?;
        Self::parse_bytes(&data)
    }
    
    /// Parse WBF from bytes
    pub fn parse_bytes(data: &[u8]) -> Result<Self, WaveformError> {
        if data.len() < 32 {
            return Err(WaveformError::InvalidFormat);
        }
        
        // Parse header
        let magic = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        
        // Check magic number (varies by vendor)
        let header = WbfHeader {
            version: u32::from_le_bytes([data[4], data[5], data[6], data[7]]),
            width: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
            height: u32::from_le_bytes([data[12], data[13], data[14], data[15]]),
            panel_id: u32::from_le_bytes([data[16], data[17], data[18], data[19]]),
            num_modes: u32::from_le_bytes([data[20], data[21], data[22], data[23]]),
            num_temps: u32::from_le_bytes([data[24], data[25], data[26], data[27]]),
        };
        
        // For now, return header only - full parsing would extract waveform data
        Ok(Self {
            header,
            waveforms: Vec::new(),
        })
    }
    
    /// Get mode count
    pub fn mode_count(&self) -> u32 {
        self.header.num_modes
    }
    
    /// Get temperature range count
    pub fn temp_count(&self) -> u32 {
        self.header.num_temps
    }
}

/// ACEP2 color waveform (Paper Pro)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Acep2Mode {
    /// Initialize
    Init = 0,
    /// Direct update monochrome
    Du = 1,
    /// Full color update
    Color = 2,
    /// Highlight
    Highlight = 3,
    /// Fast color
    FastColor = 4,
}

/// ACEP2 .eink file
#[derive(Debug)]
pub struct Acep2File {
    pub version: u32,
    pub width: u32,
    pub height: u32,
    pub num_waveforms: u32,
}

impl Acep2File {
    pub fn parse(path: &Path) -> Result<Self, WaveformError> {
        let data = std::fs::read(path)?;
        
        if data.len() < 16 {
            return Err(WaveformError::InvalidFormat);
        }
        
        Ok(Self {
            version: u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
            width: u32::from_le_bytes([data[4], data[5], data[6], data[7]]),
            height: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
            num_waveforms: u32::from_le_bytes([data[12], data[13], data[14], data[15]]),
        })
    }
}

/// List available waveform modes for device type
pub fn available_modes(device: &str) -> Vec<WaveformMode> {
    match device {
        "rm1" | "rm2" => vec![
            WaveformMode::Init,
            WaveformMode::Du,
            WaveformMode::Gc16,
            WaveformMode::Gc16Fast,
            WaveformMode::A2,
            WaveformMode::Gl16,
        ],
        "ferrari" | "paper_pro" => vec![
            WaveformMode::Init,
            WaveformMode::Du,
            WaveformMode::Gc16,
            WaveformMode::A2,
        ],
        _ => vec![WaveformMode::Init, WaveformMode::Du, WaveformMode::Gc16],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_mode_names() {
        assert_eq!(WaveformMode::Init.name(), "INIT");
        assert_eq!(WaveformMode::Du.name(), "DU");
        assert_eq!(WaveformMode::A2.name(), "A2");
    }
    
    #[test]
    fn test_mode_roundtrip() {
        for i in 0..11 {
            if let Some(mode) = WaveformMode::from_u8(i) {
                assert_eq!(mode as u8, i);
            }
        }
    }
    
    #[test]
    fn test_available_modes() {
        let rm2_modes = available_modes("rm2");
        assert!(rm2_modes.contains(&WaveformMode::A2));
    }
}
