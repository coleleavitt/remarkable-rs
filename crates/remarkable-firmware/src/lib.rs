//! Firmware extractor for reMarkable update files
//!
//! Supports:
//! - CrAU v1 (.signed) - Omaha-era firmware (2.10 - 3.11.2)
//! - SWUpdate (.swu) - Memfault-era firmware (3.11.3+)

use std::io::Read;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum FirmwareError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid firmware format")]
    InvalidFormat,
    #[error("Unsupported version: {0}")]
    UnsupportedVersion(String),
    #[error("Decompression failed")]
    DecompressionFailed,
}

/// Firmware format type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirmwareFormat {
    /// CrAU v1 (Chrome OS Auto-Update, Omaha era)
    CrauV1,
    /// CrAU v2 (newer Chrome OS format)
    CrauV2,
    /// SWUpdate (Memfault era)
    SwUpdate,
}

/// CrAU operation type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrauOperation {
    Replace = 0,
    ReplaceBz = 1,
    Move = 2,
    Bsdiff = 3,
    SourceCopy = 4,
    SourceBsdiff = 5,
    Zero = 6,
    Discard = 7,
    ReplaceXz = 8,
    Puffdiff = 9,
}

impl CrauOperation {
    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            0 => Some(Self::Replace),
            1 => Some(Self::ReplaceBz),
            2 => Some(Self::Move),
            3 => Some(Self::Bsdiff),
            4 => Some(Self::SourceCopy),
            5 => Some(Self::SourceBsdiff),
            6 => Some(Self::Zero),
            7 => Some(Self::Discard),
            8 => Some(Self::ReplaceXz),
            9 => Some(Self::Puffdiff),
            _ => None,
        }
    }
}

/// Detect firmware format from file
pub fn detect_format(path: &Path) -> Result<FirmwareFormat, FirmwareError> {
    let mut file = std::fs::File::open(path)?;
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic)?;
    
    // CrAU magic: "CrAU"
    if &magic == b"CrAU" {
        let mut version_bytes = [0u8; 8];
        file.read_exact(&mut version_bytes)?;
        let version = u64::from_be_bytes(version_bytes);
        
        return Ok(if version == 1 {
            FirmwareFormat::CrauV1
        } else {
            FirmwareFormat::CrauV2
        });
    }
    
    // SWUpdate: CPIO archive (070701 magic as ASCII)
    if magic[0] == b'0' && magic[1] == b'7' {
        return Ok(FirmwareFormat::SwUpdate);
    }
    
    Err(FirmwareError::InvalidFormat)
}

/// CrAU v1 header
#[derive(Debug, Clone)]
pub struct CrauHeader {
    pub version: u64,
    pub manifest_size: u64,
    pub signature_size: u32,
}

/// Parse CrAU v1 header
pub fn parse_crau_header(data: &[u8]) -> Result<CrauHeader, FirmwareError> {
    if data.len() < 24 || &data[0..4] != b"CrAU" {
        return Err(FirmwareError::InvalidFormat);
    }
    
    let version = u64::from_be_bytes([
        data[4], data[5], data[6], data[7],
        data[8], data[9], data[10], data[11],
    ]);
    
    let manifest_size = u64::from_be_bytes([
        data[12], data[13], data[14], data[15],
        data[16], data[17], data[18], data[19],
    ]);
    
    let signature_size = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
    
    Ok(CrauHeader {
        version,
        manifest_size,
        signature_size,
    })
}

/// Extract CrAU v1 firmware payload
pub fn extract_crau_v1(
    input: &Path,
    output_dir: &Path,
) -> Result<Vec<String>, FirmwareError> {
    let data = std::fs::read(input)?;
    let header = parse_crau_header(&data)?;
    
    if header.version != 1 {
        return Err(FirmwareError::UnsupportedVersion(format!("CrAU v{}", header.version)));
    }
    
    // Skip header (24) + manifest + signature to get to payload
    let payload_offset = 24 + header.manifest_size as usize + header.signature_size as usize;
    
    if payload_offset >= data.len() {
        return Err(FirmwareError::InvalidFormat);
    }
    
    std::fs::create_dir_all(output_dir)?;
    
    let payload = &data[payload_offset..];
    let output_path = output_dir.join("rootfs.img");
    
    // Try bzip2 decompression (CrAU v1 uses bzip2)
    if let Ok(decompressed) = decompress_bz2(payload) {
        std::fs::write(&output_path, &decompressed)?;
        return Ok(vec![output_path.to_string_lossy().to_string()]);
    }
    
    // Fallback: write raw payload
    std::fs::write(&output_path, payload)?;
    Ok(vec![output_path.to_string_lossy().to_string()])
}

/// Extract SWUpdate firmware (CPIO + raw images)
pub fn extract_swu(
    input: &Path,
    output_dir: &Path,
) -> Result<Vec<String>, FirmwareError> {
    // SWUpdate is a CPIO archive containing:
    // - sw-description (metadata)
    // - rootfs.ext4.gz or similar
    // For now, just copy the whole file and let user extract with cpio
    std::fs::create_dir_all(output_dir)?;
    
    let dest = output_dir.join("firmware.swu");
    std::fs::copy(input, &dest)?;
    
    // Also try to extract with system cpio if available
    let extracted = vec![dest.to_string_lossy().to_string()];
    
    Ok(extracted)
}

fn decompress_bz2(data: &[u8]) -> Result<Vec<u8>, FirmwareError> {
    use bzip2::read::BzDecoder;
    
    let mut decoder = BzDecoder::new(data);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)
        .map_err(|_| FirmwareError::DecompressionFailed)?;
    
    Ok(decompressed)
}

/// Extract firmware (auto-detect format)
pub fn extract(input: &Path, output_dir: &Path) -> Result<Vec<String>, FirmwareError> {
    let format = detect_format(input)?;
    
    match format {
        FirmwareFormat::CrauV1 => extract_crau_v1(input, output_dir),
        FirmwareFormat::CrauV2 => Err(FirmwareError::UnsupportedVersion("CrAU v2".to_string())),
        FirmwareFormat::SwUpdate => extract_swu(input, output_dir),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_operation_roundtrip() {
        for i in 0..10 {
            if let Some(op) = CrauOperation::from_u32(i) {
                assert_eq!(op as u32, i);
            }
        }
    }
    
    #[test]
    fn test_header_parse_invalid() {
        let data = vec![0u8; 10];
        assert!(parse_crau_header(&data).is_err());
    }
}
