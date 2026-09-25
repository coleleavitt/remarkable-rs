//! reMarkable Screen Share CLI
//!
//! A command-line tool for screen sharing with reMarkable tablets.

use std::path::PathBuf;
use std::time::Duration;

use clap::{Parser, Subcommand};
use tracing::{error, info, Level};
use tracing_subscriber::FmtSubscriber;

use remarkable_screenshare::{
    cloud::CloudConfig,
    recorder::{Recorder, RecordingConfig, RecordingFormat},
    server::{ServerConfig, WebServer, WEB_SERVER_PORT},
    usb::{UsbCapture, UsbConfig},
    viewer::ViewerSource,
    IceServer, Result, TransportConfig,
};

#[derive(Parser)]
#[command(name = "remarkable-screenshare")]
#[command(about = "Screen share viewer for reMarkable tablets")]
#[command(version)]
struct Cli {
    /// Verbosity level (-v, -vv, -vvv)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
    
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start web viewer (browser-based)
    Web {
        /// Web server port
        #[arg(short, long, default_value_t = WEB_SERVER_PORT)]
        port: u16,
        
        /// Device IP address (USB mode, used when --cloud is not given)
        #[arg(long, default_value = "10.11.99.1")]
        host: String,

        /// User token file (cloud mode)
        #[arg(long)]
        user_token: Option<PathBuf>,

        /// Cloud mode: hostname of your self-hosted remarkable-server broker
        /// (e.g. remarkable.unwrap.rs). Signaling goes over TLS to this host.
        #[arg(long)]
        cloud: Option<String>,

        /// Cloud mode: broker TLS port
        #[arg(long, default_value_t = 8883)]
        broker_port: u16,

        /// Cloud mode: user id used in signaling topics
        #[arg(long, default_value = "local-user")]
        user_id: String,

        /// Cloud mode: extra STUN/TURN server URL (repeatable), e.g. stun:stun.l.google.com:19302
        #[arg(long = "ice")]
        ice: Vec<String>,
    },
    
    /// Capture single frame
    Capture {
        /// Output file path
        #[arg(short, long, default_value = "frame.png")]
        output: PathBuf,
        
        /// Device IP address
        #[arg(long, default_value = "10.11.99.1")]
        host: String,
    },
    
    /// Start continuous capture with recording
    Record {
        /// Output directory
        #[arg(short, long, default_value = "recording")]
        output: PathBuf,
        
        /// Frames per second
        #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u32).range(1..))]
        fps: u32,
        
        /// Duration in seconds (0 for unlimited)
        #[arg(short, long, default_value_t = 0)]
        duration: u64,
        
        /// Device IP address
        #[arg(long, default_value = "10.11.99.1")]
        host: String,
        
        /// Recording format: png, jpg
        #[arg(long, default_value = "png")]
        format: String,
    },
    
    /// Test connection to device
    Test {
        /// Device IP address
        #[arg(long, default_value = "10.11.99.1")]
        host: String,
    },
    
    /// Show device info
    Info {
        /// Device IP address
        #[arg(long, default_value = "10.11.99.1")]
        host: String,
    },
    
    /// Create video from recorded frames
    Encode {
        /// Input pattern (e.g., "recording/frame_%06d.png")
        #[arg(short, long)]
        input: String,
        
        /// Output file
        #[arg(short, long)]
        output: PathBuf,
        
        /// Frames per second
        #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u32).range(1..))]
        fps: u32,
        
        /// Format: webm, mp4
        #[arg(long, default_value = "webm")]
        format: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    
    // Set up logging
    let level = match cli.verbose {
        0 => Level::INFO,
        1 => Level::DEBUG,
        _ => Level::TRACE,
    };
    
    let subscriber = FmtSubscriber::builder()
        .with_max_level(level)
        .with_target(false)
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("setting default subscriber failed");
    
    match cli.command {
        Commands::Web { port, host, user_token, cloud, broker_port, user_id, ice } => {
            if let Some(broker) = cloud {
                run_cloud_web_server(port, broker, broker_port, user_token, user_id, ice).await
            } else {
                run_usb_web_server(port, host).await
            }
        }
        Commands::Capture { output, host } => {
            capture_frame(&output, &host).await
        }
        Commands::Record { output, fps, duration, host, format } => {
            record_session(&output, fps, duration, &host, &format).await
        }
        Commands::Test { host } => {
            test_connection(&host).await
        }
        Commands::Info { host } => {
            show_device_info(&host).await
        }
        Commands::Encode { input, output, fps, format } => {
            encode_video(&input, &output, fps, &format).await
        }
    }
}

async fn run_usb_web_server(port: u16, host: String) -> Result<()> {
    let source = ViewerSource::Usb(UsbConfig::for_host(host));
    let server_config = ServerConfig { port, ..Default::default() };
    WebServer::new(server_config, source).run().await
}

async fn run_cloud_web_server(
    port: u16,
    broker: String,
    broker_port: u16,
    user_token: Option<PathBuf>,
    user_id: String,
    ice: Vec<String>,
) -> Result<()> {
    let Some(path) = user_token else {
        error!("Cloud mode requires --user-token <file> (a user token issued by your remarkable-server)");
        std::process::exit(1);
    };
    let user_token = std::fs::read_to_string(&path)?.trim().to_string();
    if user_token.is_empty() {
        error!("User token file {:?} is empty", path);
        std::process::exit(1);
    }
    info!("Cloud mode: broker {}:{}, user id {}", broker, broker_port, user_id);

    let source = ViewerSource::Cloud(CloudConfig {
        host: broker,
        port: broker_port,
        user_token,
        user_id,
        transport: TransportConfig { ice_servers: ice.into_iter().map(IceServer::url).collect(), udp_ports: None },
        timeout: Duration::from_secs(30),
    });

    let server_config = ServerConfig {
        port,
        ..Default::default()
    };
    WebServer::new(server_config, source).run().await
}

async fn capture_frame(output: &PathBuf, host: &str) -> Result<()> {
    info!("Capturing frame from {}...", host);
    
    let config = UsbConfig::for_host(host);
    
    let capture = UsbCapture::with_config(config).detected().await?;
    let frame = capture.capture_frame().await?;

    frame.save_png(output)?;
    info!("Frame saved to {:?} ({}x{})", output, frame.width, frame.height);
    
    Ok(())
}

async fn record_session(
    output: &PathBuf,
    fps: u32,
    duration: u64,
    host: &str,
    format: &str,
) -> Result<()> {
    info!("Starting recording to {:?}...", output);
    
    let recording_format = match format {
        "jpg" | "jpeg" => RecordingFormat::JpegSequence,
        _ => RecordingFormat::PngSequence,
    };
    
    let recording_config = RecordingConfig {
        format: recording_format,
        output_path: output.clone(),
        fps,
        ..Default::default()
    };
    
    let recorder = Recorder::new(recording_config);
    let tx = recorder.start().await?;
    
    let usb_config = UsbConfig::for_host(host);
    
    let capture = UsbCapture::with_config(usb_config).detected().await?;
    let mut frame_rx = capture.start_continuous(fps).await?;
    
    info!("Recording... Press Ctrl+C to stop");
    
    let start = std::time::Instant::now();
    
    tokio::select! {
        _ = async {
            while let Some(frame) = frame_rx.recv().await {
                if tx.send(frame).await.is_err() {
                    break;
                }
                if duration > 0 && start.elapsed().as_secs() >= duration {
                    break;
                }
            }
        } => {}
        _ = tokio::signal::ctrl_c() => {
            info!("Stopping recording...");
        }
    }
    
    let stats = recorder.stop().await?;
    info!(
        "Recorded {} frames in {:.1}s to {:?}",
        stats.frame_count,
        stats.duration.as_secs_f64(),
        stats.output_path
    );
    
    Ok(())
}

async fn test_connection(host: &str) -> Result<()> {
    info!("Testing connection to {}...", host);
    
    let config = UsbConfig::for_host(host);
    
    let capture = UsbCapture::with_config(config);
    
    if capture.test_connection().await? {
        info!("✓ Connection successful");
        Ok(())
    } else {
        error!("✗ Connection failed");
        std::process::exit(1);
    }
}

async fn show_device_info(host: &str) -> Result<()> {
    let config = UsbConfig::for_host(host);
    
    let capture = UsbCapture::with_config(config);
    let info = capture.get_device_info().await?;
    
    println!("Device Information:");
    println!("  Model:            {}", info.model);
    println!("  Firmware Version: {}", info.firmware_version);
    println!("  Display:          {}x{} @ {} bpp", info.width, info.height, info.depth);
    
    Ok(())
}

async fn encode_video(input: &str, output: &PathBuf, fps: u32, format: &str) -> Result<()> {
    info!("Encoding video: {} -> {:?}", input, output);
    
    remarkable_screenshare::recorder::create_video_from_sequence(input, output, fps, format).await?;
    
    info!("Video created: {:?}", output);
    Ok(())
}
