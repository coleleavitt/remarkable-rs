//! CRC32C checksum for Google Cloud Storage validation
//!
//! The reMarkable API uses Google Cloud Storage which requires CRC32C checksums
//! in the x-goog-hash header format: `crc32c={base64-encoded-value}`

use base64::{Engine as _, engine::general_purpose::STANDARD};

/// CRC32C (Castagnoli) polynomial constant
const POLY: u32 = 0x82F63B78;

/// Calculate CRC32C checksum
pub fn crc32c(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFFFFFF;
    
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ POLY;
            } else {
                crc >>= 1;
            }
        }
    }
    
    crc ^ 0xFFFFFFFF
}

/// Encode CRC32C as base64 for x-goog-hash header
pub fn crc32c_base64(data: &[u8]) -> String {
    let crc = crc32c(data);
    STANDARD.encode(crc.to_be_bytes())
}

/// Format for x-goog-hash header value
pub fn x_goog_hash_header(data: &[u8]) -> String {
    format!("crc32c={}", crc32c_base64(data))
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_crc32c_known_vector() {
        // Standard test vector: "123456789" should produce 0xE3069283
        let result = crc32c(b"123456789");
        assert_eq!(result, 0xE3069283);
    }
    
    #[test]
    fn test_crc32c_base64() {
        let result = crc32c_base64(b"123456789");
        // 0xE3069283 in big-endian base64
        assert_eq!(result, "4waSgw==");
    }
    
    #[test]
    fn test_x_goog_hash_header() {
        let header = x_goog_hash_header(b"123456789");
        assert_eq!(header, "crc32c=4waSgw==");
    }
}
