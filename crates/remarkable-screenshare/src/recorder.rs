//! Recording Support
//!
//! Record screen share sessions as PNG or JPEG image sequences (see the
//! `encode` command to turn one into a video).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{debug, info, warn};

use crate::error::{Error, Result};
use crate::session::Frame;

/// Recording format
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RecordingFormat {
    /// PNG image sequence
    PngSequence,
    /// JPEG image sequence
    JpegSequence,
}

/// Recording configuration
#[derive(Debug, Clone)]
pub struct RecordingConfig {
    pub format: RecordingFormat,
    pub output_path: PathBuf,
    pub fps: u32,
    pub quality: u8, // 1-100
}

impl Default for RecordingConfig {
    fn default() -> Self {
        Self {
            format: RecordingFormat::PngSequence,
            output_path: PathBuf::from("recording"),
            fps: 10,
            quality: 90,
        }
    }
}

/// Recording state
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RecordingState {
    Idle,
    Recording,
    Paused,
    Stopped,
}

/// Screen recorder
pub struct Recorder {
    config: RecordingConfig,
    state: Arc<RwLock<RecordingState>>,
    frame_count: Arc<Mutex<u64>>,
    start_time: Arc<Mutex<Option<Instant>>>,
}

impl Recorder {
    /// Create new recorder
    pub fn new(config: RecordingConfig) -> Self {
        Self {
            config,
            state: Arc::new(RwLock::new(RecordingState::Idle)),
            frame_count: Arc::new(Mutex::new(0)),
            start_time: Arc::new(Mutex::new(None)),
        }
    }
    
    /// Get current state
    pub async fn state(&self) -> RecordingState {
        *self.state.read().await
    }
    
    /// Get frame count
    pub async fn frame_count(&self) -> u64 {
        *self.frame_count.lock().await
    }
    
    /// Get recording duration
    pub async fn duration(&self) -> Duration {
        if let Some(start) = *self.start_time.lock().await {
            start.elapsed()
        } else {
            Duration::ZERO
        }
    }
    
    /// Start recording
    pub async fn start(&self) -> Result<mpsc::Sender<Frame>> {
        // Create output directory
        std::fs::create_dir_all(&self.config.output_path)
            .map_err(|e| Error::Recording(format!("Failed to create output directory: {}", e)))?;
        
        *self.state.write().await = RecordingState::Recording;
        *self.start_time.lock().await = Some(Instant::now());
        *self.frame_count.lock().await = 0;
        
        let (tx, mut rx) = mpsc::channel::<Frame>(32);
        
        let config = self.config.clone();
        let state = self.state.clone();
        let frame_count = self.frame_count.clone();
        
        tokio::spawn(async move {
            while let Some(frame) = rx.recv().await {
                if *state.read().await != RecordingState::Recording {
                    continue;
                }
                
                let count = {
                    let mut c = frame_count.lock().await;
                    *c += 1;
                    *c
                };
                
                // Save frame based on format
                match config.format {
                    RecordingFormat::PngSequence => {
                        let path = config.output_path.join(format!("frame_{:06}.png", count));
                        if let Err(e) = save_frame_png(&frame, &path) {
                            warn!("Failed to save frame: {}", e);
                        }
                    }
                    RecordingFormat::JpegSequence => {
                        let path = config.output_path.join(format!("frame_{:06}.jpg", count));
                        if let Err(e) = save_frame_jpeg(&frame, &path, config.quality) {
                            warn!("Failed to save frame: {}", e);
                        }
                    }
                }
                
                if count % 100 == 0 {
                    debug!("Recorded {} frames", count);
                }
            }
        });
        
        info!("Recording started: {:?}", self.config.output_path);
        Ok(tx)
    }
    
    /// Pause recording
    pub async fn pause(&self) {
        *self.state.write().await = RecordingState::Paused;
        info!("Recording paused");
    }
    
    /// Resume recording
    pub async fn resume(&self) {
        *self.state.write().await = RecordingState::Recording;
        info!("Recording resumed");
    }
    
    /// Stop recording
    pub async fn stop(&self) -> Result<RecordingStats> {
        *self.state.write().await = RecordingState::Stopped;
        
        let frame_count = *self.frame_count.lock().await;
        let duration = self.duration().await;
        
        info!(
            "Recording stopped: {} frames in {:.1}s ({:.1} fps)", 
            frame_count,
            duration.as_secs_f64(),
            frame_count as f64 / duration.as_secs_f64().max(0.001)
        );
        
        Ok(RecordingStats {
            frame_count,
            duration,
            output_path: self.config.output_path.clone(),
        })
    }
}

/// Recording statistics
#[derive(Debug, Clone)]
pub struct RecordingStats {
    pub frame_count: u64,
    pub duration: Duration,
    pub output_path: PathBuf,
}

/// Save frame as PNG
fn save_frame_png(frame: &Frame, path: &Path) -> Result<()> {
    frame.to_image()?.save(path)
        .map_err(|e| Error::Recording(format!("Failed to save PNG: {}", e)))?;
    
    Ok(())
}

/// Save frame as JPEG
fn save_frame_jpeg(frame: &Frame, path: &Path, quality: u8) -> Result<()> {
    // JPEG wants RGB either way.
    let rgb = frame.to_image()?.to_rgb8();
    
    let file = std::fs::File::create(path)
        .map_err(|e| Error::Recording(format!("Failed to create file: {}", e)))?;
    
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(file, quality);
    encoder.encode_image(&rgb)
        .map_err(|e| Error::Recording(format!("Failed to encode JPEG: {}", e)))?;
    
    Ok(())
}

/// Create video from image sequence using ffmpeg
pub async fn create_video_from_sequence(
    input_pattern: &str,
    output_path: &Path,
    fps: u32,
    format: &str,
) -> Result<()> {
    use std::process::Command;
    
    let codec = match format {
        "webm" => "libvpx-vp9",
        "mp4" => "libx264",
        _ => return Err(Error::Recording(format!("Unsupported format: {}", format))),
    };
    
    let output = Command::new("ffmpeg")
        .args([
            "-y",
            "-framerate", &fps.to_string(),
            "-i", input_pattern,
            "-c:v", codec,
            "-pix_fmt", "yuv420p",
            output_path.to_str().unwrap_or("output"),
        ])
        .output()
        .map_err(|e| Error::Recording(format!("Failed to run ffmpeg: {}", e)))?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Recording(format!("ffmpeg failed: {}", stderr)));
    }
    
    Ok(())
}
