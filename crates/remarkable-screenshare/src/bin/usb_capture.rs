//! USB Framebuffer Capture Binary
//!
//! Direct screen capture from USB-connected reMarkable device.
//! Bypasses the cloud entirely.
//!
//! Usage:
//!     usb-capture                          # Single capture
//!     usb-capture --continuous             # Continuous mode
//!     usb-capture --host 192.168.1.100     # Custom host

use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use remarkable_screenshare::{Display, PngExporter, UsbCapture, UsbConfig, GifRecorder};
use tracing::{error, info, warn};

#[derive(Parser)]
#[command(name = "usb-capture")]
#[command(about = "Capture reMarkable screen via USB")]
#[command(version)]
struct Cli {
    /// Device host
    #[arg(long, default_value = "10.11.99.1")]
    host: String,

    /// SSH user
    #[arg(long, default_value = "root")]
    user: String,

    /// SSH port
    #[arg(long, default_value = "22")]
    port: u16,

    /// SSH identity file
    #[arg(short, long)]
    identity: Option<PathBuf>,

    /// Output directory
    #[arg(short, long, default_value = "./captures")]
    output: PathBuf,

    /// Continuous capture mode
    #[arg(long)]
    continuous: bool,

    /// Capture interval in ms (continuous mode)
    #[arg(long, default_value = "500")]
    interval: u64,

    /// Record to GIF
    #[arg(long)]
    gif: bool,

    /// GIF frame delay in ms
    #[arg(long, default_value = "100")]
    gif_delay: u32,

    /// Disable display window
    #[arg(long)]
    headless: bool,

    /// Verbose output
    #[arg(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Initialize tracing
    let filter = if cli.verbose {
        "remarkable_screenshare=debug"
    } else {
        "remarkable_screenshare=info"
    };

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| filter.into()),
        )
        .init();

    // Build config
    let mut config = UsbConfig::default()
        .with_host(&cli.host);

    if let Some(ref identity) = cli.identity {
        config = config.with_identity(identity);
    }

    let capture = UsbCapture::new(config);

    // Check connection
    info!(host = %cli.host, "Checking device connection...");

    match capture.check_connection().await {
        Ok(true) => info!("Device connected"),
        Ok(false) => {
            error!("Cannot connect to device at {}", cli.host);
            error!("Make sure:");
            error!("  1. Device is connected via USB");
            error!("  2. USB networking is enabled");
            error!("  3. SSH is accessible (default: 10.11.99.1)");
            return Err("Device not reachable".into());
        }
        Err(e) => {
            error!(error = %e, "Connection check failed");
            return Err(e.into());
        }
    }

    // Get framebuffer info
    let fb_info = capture.get_fb_info().await?;
    info!(
        width = fb_info.width,
        height = fb_info.height,
        depth = fb_info.depth,
        name = %fb_info.name,
        "Framebuffer info"
    );

    // Create output directory
    std::fs::create_dir_all(&cli.output)?;

    if cli.continuous {
        // Continuous capture mode
        let mut display = if !cli.headless {
            Some(Display::with_dimensions(
                "reMarkable USB Capture",
                fb_info.width as usize,
                fb_info.height as usize,
            )?)
        } else {
            None
        };

        let _exporter = PngExporter::new(&cli.output, "usb")?;

        let mut recorder = if cli.gif {
            let path = cli.output.join("recording.gif");
            Some(GifRecorder::new(path, cli.gif_delay)
                .with_dimensions(fb_info.width, fb_info.height))
        } else {
            None
        };

        info!(
            interval_ms = cli.interval,
            "Starting continuous capture (press ESC to stop)"
        );

        let mut frame_count = 0u64;
        let start = std::time::Instant::now();

        loop {
            match capture.capture_frame().await {
                Ok(frame) => {
                    frame_count += 1;

                    // Update display
                    if let Some(ref mut disp) = display {
                        if disp.should_close() {
                            info!("Window closed");
                            break;
                        }
                        if let Err(e) = disp.update_grayscale(&frame) {
                            warn!(error = %e, "Display update failed");
                        }
                    }

                    // Record to GIF
                    if let Some(ref mut rec) = recorder {
                        rec.add_frame(&frame);
                    }

                    // Log progress every 10 frames
                    if frame_count % 10 == 0 {
                        let elapsed = start.elapsed().as_secs_f64();
                        let fps = frame_count as f64 / elapsed;
                        info!(
                            frames = frame_count,
                            fps = format!("{:.1}", fps),
                            "Capture progress"
                        );
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Capture error");
                }
            }

            tokio::time::sleep(Duration::from_millis(cli.interval)).await;
        }

        // Finalize GIF
        if let Some(rec) = recorder {
            info!("Finalizing GIF...");
            rec.finalize()?;
        }

        info!(
            frames = frame_count,
            duration_secs = start.elapsed().as_secs(),
            "Capture session ended"
        );
    } else {
        // Single capture
        info!("Capturing single frame...");
        let frame = capture.capture_frame().await?;

        let path = cli.output.join("capture.png");
        remarkable_screenshare::export::save_frame_png(
            &frame,
            fb_info.width,
            fb_info.height,
            &path,
        )?;

        info!(path = %path.display(), "Saved capture");

        // Show in display if not headless
        if !cli.headless {
            let mut display = Display::with_dimensions(
                "reMarkable USB Capture (press ESC to close)",
                fb_info.width as usize,
                fb_info.height as usize,
            )?;

            display.update_grayscale(&frame)?;

            while !display.should_close() {
                display.wait();
            }
        }
    }

    Ok(())
}
