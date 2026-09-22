//! Frame Export - PNG and GIF output
//!
//! Supports:
//! - Single frame PNG export
//! - Animated GIF recording
//! - Frame sequence export

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use std::time::Instant;

use image::codecs::gif::{GifEncoder, Repeat};
use image::{Frame, ImageBuffer, Luma, RgbaImage};
use thiserror::Error;
use tracing::{debug, info};

use crate::rfb::{FB_HEIGHT, FB_WIDTH};

/// Export errors
#[derive(Error, Debug)]
pub enum ExportError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Image error: {0}")]
    Image(#[from] image::ImageError),

    #[error("Invalid dimensions: {width}x{height}")]
    InvalidDimensions { width: u32, height: u32 },

    #[error("Encoder error: {0}")]
    Encoder(String),
}

/// PNG frame exporter
pub struct PngExporter {
    output_dir: std::path::PathBuf,
    frame_count: u64,
    prefix: String,
}

impl PngExporter {
    /// Create new PNG exporter
    pub fn new<P: AsRef<Path>>(output_dir: P, prefix: &str) -> Result<Self, ExportError> {
        let output_dir = output_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&output_dir)?;

        Ok(Self {
            output_dir,
            frame_count: 0,
            prefix: prefix.to_string(),
        })
    }

    /// Export grayscale frame to PNG
    pub fn export_grayscale(
        &mut self,
        data: &[u8],
        width: u32,
        height: u32,
    ) -> Result<std::path::PathBuf, ExportError> {
        if data.len() != (width * height) as usize {
            return Err(ExportError::InvalidDimensions { width, height });
        }

        // Create grayscale image (inverted: 0=white, 255=black on device)
        let img: ImageBuffer<Luma<u8>, Vec<u8>> = ImageBuffer::from_fn(width, height, |x, y| {
            let idx = (y * width + x) as usize;
            Luma([255 - data[idx]])
        });

        let filename = format!(
            "{}_{:06}.png",
            self.prefix, self.frame_count
        );
        let path = self.output_dir.join(&filename);

        img.save(&path)?;
        self.frame_count += 1;

        debug!(path = %path.display(), "Exported PNG frame");

        Ok(path)
    }

    /// Export current timestamp frame
    pub fn export_timestamped(
        &mut self,
        data: &[u8],
        width: u32,
        height: u32,
    ) -> Result<std::path::PathBuf, ExportError> {
        if data.len() != (width * height) as usize {
            return Err(ExportError::InvalidDimensions { width, height });
        }

        let img: ImageBuffer<Luma<u8>, Vec<u8>> = ImageBuffer::from_fn(width, height, |x, y| {
            let idx = (y * width + x) as usize;
            Luma([255 - data[idx]])
        });

        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let filename = format!("{}_{}.png", self.prefix, timestamp);
        let path = self.output_dir.join(&filename);

        img.save(&path)?;

        info!(path = %path.display(), "Exported timestamped PNG");

        Ok(path)
    }

    /// Get number of exported frames
    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }
}

/// GIF recorder for animated screen capture
pub struct GifRecorder {
    output_path: std::path::PathBuf,
    frames: Vec<(Vec<u8>, u32, u32)>,
    frame_delay_ms: u32,
    start_time: Instant,
    last_frame_time: Instant,
    width: u32,
    height: u32,
}

impl GifRecorder {
    /// Create new GIF recorder
    pub fn new<P: AsRef<Path>>(output_path: P, frame_delay_ms: u32) -> Self {
        let now = Instant::now();
        Self {
            output_path: output_path.as_ref().to_path_buf(),
            frames: Vec::new(),
            frame_delay_ms,
            start_time: now,
            last_frame_time: now,
            width: FB_WIDTH as u32,
            height: FB_HEIGHT as u32,
        }
    }

    /// Set dimensions
    pub fn with_dimensions(mut self, width: u32, height: u32) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// Add frame if enough time has passed
    pub fn add_frame(&mut self, data: &[u8]) -> bool {
        let now = Instant::now();
        if now.duration_since(self.last_frame_time).as_millis() >= self.frame_delay_ms as u128 {
            self.frames.push((data.to_vec(), self.width, self.height));
            self.last_frame_time = now;
            true
        } else {
            false
        }
    }

    /// Force add frame
    pub fn add_frame_force(&mut self, data: &[u8]) {
        self.frames.push((data.to_vec(), self.width, self.height));
        self.last_frame_time = Instant::now();
    }

    /// Get frame count
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Get recording duration
    pub fn duration_secs(&self) -> f64 {
        self.start_time.elapsed().as_secs_f64()
    }

    /// Finalize and write GIF
    pub fn finalize(self) -> Result<std::path::PathBuf, ExportError> {
        info!(
            frames = self.frames.len(),
            duration_secs = self.duration_secs(),
            path = %self.output_path.display(),
            "Writing GIF"
        );

        let file = File::create(&self.output_path)?;
        let writer = BufWriter::new(file);
        let mut encoder = GifEncoder::new(writer);
        encoder.set_repeat(Repeat::Infinite).map_err(|e| ExportError::Encoder(e.to_string()))?;

        for (data, width, height) in &self.frames {
            // Convert grayscale to RGBA
            let mut rgba = RgbaImage::new(*width, *height);
            for (i, &gray) in data.iter().enumerate() {
                let x = (i as u32) % width;
                let y = (i as u32) / width;
                let g = 255 - gray; // Invert
                rgba.put_pixel(x, y, image::Rgba([g, g, g, 255]));
            }

            // Frame delay in centiseconds
            let delay = image::Delay::from_numer_denom_ms(self.frame_delay_ms, 1);
            let frame = Frame::from_parts(rgba, 0, 0, delay);
            encoder.encode_frame(frame).map_err(|e| ExportError::Encoder(e.to_string()))?;
        }

        info!(path = %self.output_path.display(), "GIF written successfully");

        Ok(self.output_path)
    }
}

/// Export a single frame to PNG
pub fn save_frame_png<P: AsRef<Path>>(
    data: &[u8],
    width: u32,
    height: u32,
    path: P,
) -> Result<(), ExportError> {
    if data.len() != (width * height) as usize {
        return Err(ExportError::InvalidDimensions { width, height });
    }

    let img: ImageBuffer<Luma<u8>, Vec<u8>> = ImageBuffer::from_fn(width, height, |x, y| {
        let idx = (y * width + x) as usize;
        Luma([255 - data[idx]])
    });

    img.save(path.as_ref())?;
    Ok(())
}

/// Export RGB32 buffer to PNG
pub fn save_rgb32_png<P: AsRef<Path>>(
    data: &[u32],
    width: u32,
    height: u32,
    path: P,
) -> Result<(), ExportError> {
    if data.len() != (width * height) as usize {
        return Err(ExportError::InvalidDimensions { width, height });
    }

    let mut rgba = RgbaImage::new(width, height);
    for (i, &pixel) in data.iter().enumerate() {
        let x = (i as u32) % width;
        let y = (i as u32) / width;
        let r = ((pixel >> 16) & 0xFF) as u8;
        let g = ((pixel >> 8) & 0xFF) as u8;
        let b = (pixel & 0xFF) as u8;
        rgba.put_pixel(x, y, image::Rgba([r, g, b, 255]));
    }

    rgba.save(path.as_ref())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_png_export() {
        let dir = tempdir().unwrap();
        let mut exporter = PngExporter::new(dir.path(), "test").unwrap();

        let data = vec![128u8; 100 * 100];
        let path = exporter.export_grayscale(&data, 100, 100).unwrap();

        assert!(path.exists());
        assert_eq!(exporter.frame_count(), 1);
    }

    #[test]
    fn test_gif_recorder() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.gif");
        let mut recorder = GifRecorder::new(&path, 100)
            .with_dimensions(10, 10);

        let data = vec![128u8; 100];
        recorder.add_frame_force(&data);
        recorder.add_frame_force(&data);

        assert_eq!(recorder.frame_count(), 2);

        let result = recorder.finalize().unwrap();
        assert!(result.exists());
    }
}
