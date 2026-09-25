//! MQTT CLI tool for reMarkable debugging
//!
//! # Commands
//!
//! - `listen` - Listen to MQTT notifications
//! - `sync-watch` - Watch for sync events
//! - `screenshare` - Screen share signaling
//! - `info` - Show connection info
//! - `tokens` - Extract tokens from device via SSH
//!
//! # Usage
//!
//! ```bash
//! # Listen to all notifications
//! remarkable-mqtt listen
//!
//! # Watch sync events only
//! remarkable-mqtt sync-watch
//!
//! # Extract tokens from device
//! remarkable-mqtt tokens --ssh 10.11.99.1
//!
//! # Show connection info
//! remarkable-mqtt info
//! ```

use std::path::PathBuf;
use std::time::Duration;
use tracing::{error, info};

use remarkable_mqtt::{
    ssh::{self, SshConfig},
    sync_events::SyncState,
    MqttClient, MqttConfig, MqttEvent, ReconnectingClient,
};

#[derive(Debug, Clone)]
enum Command {
    /// Listen to MQTT notifications
    Listen {
        /// Timeout in seconds (0 = infinite)
        timeout: u64,
        /// Show raw payloads
        raw: bool,
    },
    /// Watch sync events
    SyncWatch {
        /// Timeout in seconds (0 = infinite)
        timeout: u64,
    },
    /// Extract tokens from device via SSH
    Tokens {
        /// Device host (default: 10.11.99.1)
        host: String,
        /// Save tokens to directory
        save_to: Option<PathBuf>,
    },
    /// Show connection info
    Info,
    /// Test connection
    Test,
}

fn parse_args() -> Command {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        return Command::Listen {
            timeout: 0,
            raw: false,
        };
    }

    match args[1].as_str() {
        "listen" => {
            let timeout = args
                .iter()
                .position(|a| a == "--timeout")
                .and_then(|i| args.get(i + 1))
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let raw = args.iter().any(|a| a == "--raw");
            Command::Listen { timeout, raw }
        }
        "sync-watch" | "sync" => {
            let timeout = args
                .iter()
                .position(|a| a == "--timeout")
                .and_then(|i| args.get(i + 1))
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            Command::SyncWatch { timeout }
        }
        "tokens" => {
            let host = args
                .iter()
                .position(|a| a == "--ssh" || a == "--host")
                .and_then(|i| args.get(i + 1))
                .cloned()
                .unwrap_or_else(|| "10.11.99.1".to_string());
            let save_to = args
                .iter()
                .position(|a| a == "--save" || a == "-o")
                .and_then(|i| args.get(i + 1))
                .map(PathBuf::from);
            Command::Tokens { host, save_to }
        }
        "info" => Command::Info,
        "test" => Command::Test,
        _ => {
            eprintln!("Usage: remarkable-mqtt <command>");
            eprintln!();
            eprintln!("Commands:");
            eprintln!("  listen     Listen to MQTT notifications");
            eprintln!("  sync-watch Watch sync events");
            eprintln!("  tokens     Extract tokens from device via SSH");
            eprintln!("  info       Show connection info");
            eprintln!("  test       Test connection");
            eprintln!();
            eprintln!("Options:");
            eprintln!("  --timeout <secs>  Timeout (0 = infinite)");
            eprintln!("  --raw             Show raw payloads");
            eprintln!("  --ssh <host>      Device SSH host");
            eprintln!("  --save <dir>      Save tokens to directory");
            std::process::exit(1);
        }
    }
}

async fn load_tokens() -> Result<(String, String), Box<dyn std::error::Error>> {
    // Try environment variables first
    if let (Ok(device), Ok(user)) = (
        std::env::var("REMARKABLE_DEVICE_TOKEN"),
        std::env::var("REMARKABLE_USER_TOKEN"),
    ) {
        info!("Using tokens from environment");
        return Ok((device, user));
    }

    // Try captured_tokens directory
    let tokens_dir = PathBuf::from(std::env::var("HOME")?)
        .join("SiteResearch/remarkable/captured_tokens");

    if tokens_dir.exists() {
        let device_path = tokens_dir.join("device_token_actual.txt");
        let user_path = tokens_dir.join("user_token_actual.txt");

        if device_path.exists() && user_path.exists() {
            let device = std::fs::read_to_string(&device_path)?.trim().to_string();
            let user = std::fs::read_to_string(&user_path)?.trim().to_string();
            info!(dir = %tokens_dir.display(), "Loaded tokens from file");
            return Ok((device, user));
        }
    }

    Err("No tokens found. Set REMARKABLE_DEVICE_TOKEN/REMARKABLE_USER_TOKEN or run 'tokens' command".into())
}

async fn cmd_listen(timeout: u64, raw: bool) -> Result<(), Box<dyn std::error::Error>> {
    let (device_token, user_token) = load_tokens().await?;
    let config = MqttConfig::from_tokens(&device_token, &user_token)?;

    info!(
        user_id = %config.user_id,
        client_id = %config.client_id,
        broker = %config.broker,
        "Connecting to MQTT broker"
    );

    let mut client = ReconnectingClient::new(config);
    client.connect().await?;
    client.subscribe_default().await?;

    info!("Listening for notifications...");

    let start = std::time::Instant::now();
    let timeout_duration = if timeout > 0 {
        Some(Duration::from_secs(timeout))
    } else {
        None
    };

    loop {
        if let Some(t) = timeout_duration {
            if start.elapsed() > t {
                info!("Timeout reached");
                break;
            }
        }

        match tokio::time::timeout(Duration::from_secs(5), client.poll()).await {
            Ok(Ok(event)) => {
                print_event(&event, raw);
            }
            Ok(Err(e)) => {
                error!(?e, "Poll error");
            }
            Err(_) => {
                // Timeout, continue
            }
        }
    }

    client.disconnect().await?;
    Ok(())
}

fn print_event(event: &MqttEvent, raw: bool) {
    match event {
        MqttEvent::Connected => {
            println!("✓ Connected");
        }
        MqttEvent::Disconnected => {
            println!("✗ Disconnected");
        }
        MqttEvent::Subscribed(topic) => {
            println!("⊕ Subscribed: {}", topic);
        }
        MqttEvent::Notification { topic, notification } => {
            println!(
                "📬 Notification [{}] from {}: {}",
                topic, notification.source_device_id, notification.message
            );
        }
        MqttEvent::SyncComplete { topic, sync } => {
            println!(
                "🔄 Sync Complete [{}] gen={} from {}",
                topic, sync.generation, sync.source_device_id
            );
        }
        MqttEvent::Raw { topic, payload } => {
            if raw {
                println!(
                    "📦 Raw [{}]: {}",
                    topic,
                    String::from_utf8_lossy(payload)
                );
            } else {
                println!("📦 Raw [{}]: {} bytes", topic, payload.len());
            }
        }
        MqttEvent::Ping => {
            println!("🏓 Ping");
        }
    }
}

async fn cmd_sync_watch(timeout: u64) -> Result<(), Box<dyn std::error::Error>> {
    let (device_token, user_token) = load_tokens().await?;
    let config = MqttConfig::from_tokens(&device_token, &user_token)?;

    info!(
        user_id = %config.user_id,
        "Starting sync event watcher"
    );

    let mut client = ReconnectingClient::new(config);
    client.connect().await?;
    client.subscribe_default().await?;

    let mut state = SyncState::default();

    println!("Watching for sync events...");

    let start = std::time::Instant::now();
    let timeout_duration = if timeout > 0 {
        Some(Duration::from_secs(timeout))
    } else {
        None
    };

    loop {
        if let Some(t) = timeout_duration {
            if start.elapsed() > t {
                info!("Timeout reached");
                break;
            }
        }

        match tokio::time::timeout(Duration::from_secs(5), client.poll()).await {
            Ok(Ok(MqttEvent::SyncComplete { sync, .. })) => {
                if let Some(action) = state.process_event(&sync) {
                    match action {
                        remarkable_mqtt::sync_events::SyncAction::FetchRoot { generation, source_device } => {
                            println!(
                                "🔄 Generation {} from {} - fetch new root",
                                generation, source_device
                            );
                        }
                        remarkable_mqtt::sync_events::SyncAction::IncrementalSync { changed_docs } => {
                            println!("📝 Incremental sync: {} docs changed", changed_docs.len());
                        }
                        remarkable_mqtt::sync_events::SyncAction::FullResync { reason } => {
                            println!("⚠️ Full resync needed: {}", reason);
                        }
                    }
                }
            }
            Ok(Ok(_)) => {
                // Other events, ignore
            }
            Ok(Err(e)) => {
                error!(?e, "Poll error");
            }
            Err(_) => {
                // Timeout, continue
            }
        }
    }

    client.disconnect().await?;
    Ok(())
}

async fn cmd_tokens(host: String, save_to: Option<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    let config = SshConfig::default().with_host(&host);

    println!("Checking device at {}...", host);

    if !ssh::check_device_reachable(&config).await {
        return Err(format!("Device not reachable at {}", host).into());
    }

    println!("Extracting tokens...");
    let tokens = ssh::extract_device_tokens(&config).await?;

    println!();
    println!("Device Token ({} chars):", tokens.device_token.len());
    println!("  {}...", &tokens.device_token[..50.min(tokens.device_token.len())]);
    println!();
    println!("User Token ({} chars):", tokens.user_token.len());
    println!("  {}...", &tokens.user_token[..50.min(tokens.user_token.len())]);

    if let Some(dir) = save_to {
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("device_token.txt"), &tokens.device_token)?;
        std::fs::write(dir.join("user_token.txt"), &tokens.user_token)?;
        println!();
        println!("Saved tokens to {}", dir.display());
    }

    // Try to get device info
    println!();
    println!("Device Info:");
    if let Ok(serial) = ssh::get_device_serial(&config).await {
        println!("  Serial: {}", serial);
    }
    if let Ok(model) = ssh::get_device_model(&config).await {
        println!("  Model: {}", model);
    }

    Ok(())
}

async fn cmd_info() -> Result<(), Box<dyn std::error::Error>> {
    let (device_token, user_token) = load_tokens().await?;
    let config = MqttConfig::from_tokens(&device_token, &user_token)?;

    println!("MQTT Connection Info");
    println!("====================");
    println!();
    println!("Broker:    wss://{}:{}", config.broker, config.port);
    println!("User ID:   {}", config.user_id);
    println!("Client ID: {}", config.client_id);
    println!();
    println!("Device Token: {}... ({} chars)", &config.device_token[..30], config.device_token.len());
    println!("User Token:   {}... ({} chars)", &config.user_token[..30], config.user_token.len());
    println!();
    println!("Topics:");
    println!("  user/{}/sync", config.user_id);
    println!("  user/{}/client/{}/notifications", config.user_id, config.client_id);
    println!("  user/{}/client/{}/sync", config.user_id, config.client_id);
    println!(
        "  remarkable/screenshare/signaling/user/{}/client/{}/signaling (screen share, publish)",
        config.user_id, config.client_id
    );

    Ok(())
}

async fn cmd_test() -> Result<(), Box<dyn std::error::Error>> {
    let (device_token, user_token) = load_tokens().await?;
    let config = MqttConfig::from_tokens(&device_token, &user_token)?;

    println!("Testing MQTT connection...");
    println!();

    let mut client = MqttClient::new(config.clone());

    match client.connect().await {
        Ok(()) => {
            println!("✓ Connected to {}", config.broker);
        }
        Err(e) => {
            println!("✗ Connection failed: {}", e);
            return Err(e.into());
        }
    }

    match client.subscribe_default().await {
        Ok(()) => {
            println!("✓ Subscribed to topics");
        }
        Err(e) => {
            println!("✗ Subscribe failed: {}", e);
            return Err(e.into());
        }
    }

    println!();
    println!("Waiting for events (10 seconds)...");

    let mut events = 0;
    let start = std::time::Instant::now();

    while start.elapsed() < Duration::from_secs(10) {
        match tokio::time::timeout(Duration::from_secs(2), client.poll()).await {
            Ok(Ok(event)) => {
                events += 1;
                println!("  Event {}: {:?}", events, event);
            }
            Ok(Err(e)) => {
                println!("  Error: {}", e);
            }
            Err(_) => {
                // Timeout, continue
            }
        }
    }

    client.disconnect().await?;
    println!();
    println!("✓ Test complete ({} events received)", events);

    Ok(())
}

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("remarkable_mqtt=info".parse().unwrap())
                .add_directive("rumqttc=warn".parse().unwrap()),
        )
        .init();

    let cmd = parse_args();

    let result = match cmd {
        Command::Listen { timeout, raw } => cmd_listen(timeout, raw).await,
        Command::SyncWatch { timeout } => cmd_sync_watch(timeout).await,
        Command::Tokens { host, save_to } => cmd_tokens(host, save_to).await,
        Command::Info => cmd_info().await,
        Command::Test => cmd_test().await,
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
