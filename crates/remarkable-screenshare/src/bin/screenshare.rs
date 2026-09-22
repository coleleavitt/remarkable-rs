//! reMarkable Screen Share CLI
//!
//! Usage:
//!     screenshare --device-token <path> --user-token <path>
//!     screenshare --tokens-dir ~/SiteResearch/remarkable/captured_tokens
//!
//! The device must have screen share enabled for cloud mode to work.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use remarkable_screenshare::{ClientConfig, ScreenShareClient};
use tracing::{error, info};

#[derive(Parser)]
#[command(name = "screenshare")]
#[command(about = "reMarkable screen share viewer")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Device token file path
    #[arg(long)]
    device_token: Option<PathBuf>,

    /// User token file path
    #[arg(long)]
    user_token: Option<PathBuf>,

    /// Tokens directory (contains device_token_actual.txt and user_token_actual.txt)
    #[arg(long)]
    tokens_dir: Option<PathBuf>,

    /// Output directory for frames/recordings
    #[arg(short, long, default_value = "./frames")]
    output: PathBuf,

    /// Disable display window (headless mode)
    #[arg(long)]
    headless: bool,

    /// Record to GIF
    #[arg(long)]
    gif: bool,

    /// GIF frame delay in ms
    #[arg(long, default_value = "100")]
    gif_delay: u32,

    /// Save PNG snapshots
    #[arg(long)]
    snapshots: bool,

    /// Snapshot interval in seconds
    #[arg(long, default_value = "60")]
    snapshot_interval: u64,

    /// Verbose output
    #[arg(short, long)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Start cloud screen share (MQTT + WebRTC)
    Cloud(CloudArgs),

    /// Capture via USB (SSH to device)
    Usb(UsbArgs),

    /// Show protocol info
    Info,
}

#[derive(Args)]
struct CloudArgs {
    /// Device token file
    #[arg(long)]
    device_token: Option<PathBuf>,

    /// User token file
    #[arg(long)]
    user_token: Option<PathBuf>,
}

#[derive(Args)]
struct UsbArgs {
    /// Device host (default: 10.11.99.1)
    #[arg(long, default_value = "10.11.99.1")]
    host: String,

    /// SSH user (default: root)
    #[arg(long, default_value = "root")]
    user: String,

    /// SSH port
    #[arg(long, default_value = "22")]
    port: u16,

    /// SSH identity file
    #[arg(short, long)]
    identity: Option<PathBuf>,

    /// Continuous capture mode
    #[arg(long)]
    continuous: bool,

    /// Capture interval in ms
    #[arg(long, default_value = "500")]
    interval: u64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Initialize tracing
    let filter = if cli.verbose {
        "remarkable_screenshare=debug,rumqttc=debug,webrtc=debug"
    } else {
        "remarkable_screenshare=info"
    };

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| filter.into()),
        )
        .init();

    match cli.command {
        Some(Commands::Info) => {
            println!("reMarkable Screen Share Protocol");
            println!();
            println!("Cloud Mode:");
            println!("  - MQTT Broker: vernemq-prod.cloud.remarkable.engineering:443");
            println!("  - Signaling: MQTT WebSocket + TLS");
            println!("  - Transport: WebRTC DataChannel");
            println!("  - Protocol: RFB 3.8");
            println!();
            println!("Framebuffer:");
            println!("  - Resolution: 1872x1404");
            println!("  - Format: 8-bit grayscale");
            println!("  - Size: {} bytes", 1872 * 1404);
            println!();
            println!("USB Mode:");
            println!("  - Host: 10.11.99.1 (USB connection)");
            println!("  - Protocol: SSH + cat /dev/fb0");
            println!("  - Requirements: SSH access to device");
            Ok(())
        }

        Some(Commands::Usb(args)) => {
            use remarkable_screenshare::{UsbCapture, UsbConfig};

            let mut config = UsbConfig::default()
                .with_host(&args.host);

            if let Some(ref identity) = args.identity {
                config = config.with_identity(identity);
            }

            let capture = UsbCapture::new(config);

            info!(host = %args.host, "Checking device connection...");

            if !capture.check_connection().await? {
                error!("Cannot connect to device at {}", args.host);
                return Err("Device not reachable".into());
            }

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

            if args.continuous {
                use remarkable_screenshare::{Display, PngExporter};
                use std::time::Duration;

                let mut display = if !cli.headless {
                    Some(Display::with_dimensions(
                        "reMarkable USB Capture",
                        fb_info.width as usize,
                        fb_info.height as usize,
                    )?)
                } else {
                    None
                };

                let mut exporter = PngExporter::new(&cli.output, "usb")?;

                info!(
                    interval_ms = args.interval,
                    "Starting continuous capture"
                );

                loop {
                    match capture.capture_frame().await {
                        Ok(frame) => {
                            if let Some(ref mut disp) = display {
                                if disp.should_close() {
                                    break;
                                }
                                disp.update_grayscale(&frame)?;
                            }

                            if cli.snapshots {
                                exporter.export_grayscale(
                                    &frame,
                                    fb_info.width,
                                    fb_info.height,
                                )?;
                            }
                        }
                        Err(e) => {
                            error!(error = %e, "Capture error");
                        }
                    }

                    tokio::time::sleep(Duration::from_millis(args.interval)).await;
                }
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
            }

            Ok(())
        }

        Some(Commands::Cloud(ref cloud_args)) => {
            run_cloud(&cli, Some(cloud_args)).await
        }

        None => {
            run_cloud(&cli, None).await
        }
    }
}

async fn run_cloud(cli: &Cli, cloud_args: Option<&CloudArgs>) -> Result<(), Box<dyn std::error::Error>> {
    // Resolve token paths
    let (device_token_path, user_token_path) = resolve_token_paths(cli, cloud_args)?;

    info!(
        device_token = %device_token_path.display(),
        user_token = %user_token_path.display(),
        "Loading tokens"
    );

    let mut config = ClientConfig::from_files(&device_token_path, &user_token_path)?;
    config = config.with_display(!cli.headless);

    if cli.gif {
        config = config.with_gif(cli.gif_delay);
    }

    if cli.snapshots {
        config = config.with_snapshots(cli.snapshot_interval);
    }

    config = config.with_output(&cli.output);

    // Create output directory
    std::fs::create_dir_all(&cli.output)?;

    let mut client = ScreenShareClient::new(config);

    info!("Starting screen share client...");
    client.run().await?;

    Ok(())
}

fn resolve_token_paths(
    cli: &Cli,
    cloud_args: Option<&CloudArgs>,
) -> Result<(PathBuf, PathBuf), Box<dyn std::error::Error>> {
    let cloud_device = cloud_args.and_then(|a| a.device_token.clone());
    let cloud_user = cloud_args.and_then(|a| a.user_token.clone());

    let device_token_path = cloud_device
        .or_else(|| cli.device_token.clone())
        .or_else(|| {
            cli.tokens_dir
                .as_ref()
                .map(|d| d.join("device_token_actual.txt"))
        })
        .ok_or("Missing device token path. Use --device-token or --tokens-dir")?;

    let user_token_path = cloud_user
        .or_else(|| cli.user_token.clone())
        .or_else(|| {
            cli.tokens_dir
                .as_ref()
                .map(|d| d.join("user_token_actual.txt"))
        })
        .ok_or("Missing user token path. Use --user-token or --tokens-dir")?;

    if !device_token_path.exists() {
        return Err(format!("Device token file not found: {}", device_token_path.display()).into());
    }

    if !user_token_path.exists() {
        return Err(format!("User token file not found: {}", user_token_path.display()).into());
    }

    Ok((device_token_path, user_token_path))
}
