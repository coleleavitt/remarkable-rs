//! reMarkable CLI - Complete tablet management tool
//!
//! # Commands
//! 
//! ## File Operations
//! - `info`: Display .rm file info
//! - `parse`: Parse .rm file and export to SVG
//! - `export-all`: Batch export .rm files to SVG
//! - `annotate-pdf`: Add .rm strokes to PDF
//!
//! ## Sync Operations
//! - `sync list`: List documents in cloud
//! - `sync pull`: Download documents
//! - `sync push`: Upload documents
//! - `sync status`: Show sync state
//! - `sync root`: Show root hash
//!
//! ## Backup Operations
//! - `backup create`: Full cloud backup
//! - `backup restore`: Restore from backup
//! - `backup list`: List local backups
//!
//! ## Device Operations
//! - `device info`: Device status via USB/SSH
//! - `device screenshot`: Capture screen
//! - `device usb list`: List documents via USB
//! - `device usb upload`: Upload via USB
//! - `device ssh exec`: Run SSH command
//!
//! ## MQTT Operations
//! - `mqtt listen`: Monitor real-time events
//!
//! ## Server Operations
//! - `server start`: Start local sync server
//!
//! # Configuration
//!
//! Tokens are read from:
//! - `~/.config/remarkable/device_token.txt`
//! - `~/.config/remarkable/user_token.txt`
//!
//! Override with `--device-token` and `--user-token` flags.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use clap::{Parser, Subcommand};
use colored::Colorize;
use directories::ProjectDirs;
use indicatif::{ProgressBar, ProgressStyle};
use remarkable_lines::{parse_rm_file, strokes_to_svg};
use remarkable_mqtt::{MqttClient, MqttConfig, MqttEvent};
use remarkable_pdf::PdfAnnotator;
use remarkable_sync::SyncClient;
use remarkable_usb::UsbClient;
use serde::{Deserialize, Serialize};
use reqwest;
use uuid::Uuid;
use tabled::{Table, Tabled};
use thiserror::Error;
use tracing::{debug, error, info, warn};
use tracing_subscriber::EnvFilter;

/// CLI error type
#[derive(Error, Debug)]
pub enum CliError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Sync error: {0}")]
    Sync(#[from] remarkable_sync::SyncError),
    #[error("MQTT error: {0}")]
    Mqtt(#[from] remarkable_mqtt::MqttError),
    #[error("USB error: {0}")]
    Usb(#[from] remarkable_usb::UsbError),
    #[error("Screen error: {0}")]
    Screen(#[from] remarkable_screenshare::Error),
    #[error("Lines error: {0}")]
    Lines(#[from] remarkable_lines::LinesError),
    #[error("PDF error: {0}")]
    Pdf(#[from] remarkable_pdf::PdfError),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Token not found: {0}")]
    TokenNotFound(String),
    #[error("Device not connected")]
    DeviceNotConnected,
    #[error("Backup not found: {0}")]
    BackupNotFound(String),
    #[error("{0}")]
    Other(String),
}

type Result<T> = std::result::Result<T, CliError>;

/// Default config directory
fn config_dir() -> PathBuf {
    if let Some(proj_dirs) = ProjectDirs::from("com", "remarkable", "remarkable-cli") {
        proj_dirs.config_dir().to_path_buf()
    } else {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".config/remarkable")
    }
}

/// Load token from file or return error
fn load_token(path: &PathBuf) -> Result<String> {
    std::fs::read_to_string(path)
        .map(|s| s.trim().to_string())
        .map_err(|_| CliError::TokenNotFound(path.display().to_string()))
}

#[derive(Parser)]
#[command(name = "remarkable")]
#[command(author, version, about = "Complete CLI for reMarkable tablet management", long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    /// Device token file path
    #[arg(long, global = true, env = "REMARKABLE_DEVICE_TOKEN_FILE")]
    device_token: Option<PathBuf>,
    
    /// User token file path
    #[arg(long, global = true, env = "REMARKABLE_USER_TOKEN_FILE")]
    user_token: Option<PathBuf>,
    
    /// Enable verbose output
    #[arg(short, long, global = true)]
    verbose: bool,
    
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Display .rm file information
    Info {
        /// Input .rm file
        #[arg(short, long)]
        file: PathBuf,
    },
    
    /// Parse .rm file and optionally export to SVG
    Parse {
        /// Input .rm file
        #[arg(short, long)]
        file: PathBuf,
        
        /// Output SVG file
        #[arg(short, long)]
        svg: Option<PathBuf>,
    },
    
    /// Annotate a PDF with .rm strokes
    AnnotatePdf {
        /// Input PDF file
        #[arg(short, long)]
        pdf: PathBuf,
        
        /// Directory containing .rm files for each page
        #[arg(short, long)]
        rm_dir: PathBuf,
        
        /// Output PDF file
        #[arg(short, long)]
        output: PathBuf,
    },
    
    /// Export all .rm files in a directory to SVG
    ExportAll {
        /// Input directory containing .rm files
        #[arg(short, long)]
        input: PathBuf,
        
        /// Output directory for SVG files
        #[arg(short, long)]
        output: PathBuf,
        
        /// Continue on errors
        #[arg(long, default_value = "false")]
        ignore_errors: bool,
    },
    
    /// Sync operations with reMarkable cloud
    Sync {
        #[command(subcommand)]
        action: SyncAction,
    },
    
    /// Backup operations
    Backup {
        #[command(subcommand)]
        action: BackupAction,
    },
    
    /// Device operations via USB or SSH
    Device {
        #[command(subcommand)]
        action: DeviceAction,
    },
    
    /// MQTT real-time event monitoring
    Mqtt {
        #[command(subcommand)]
        action: MqttAction,
    },
    
    /// Local sync server
    Server {
        #[command(subcommand)]
        action: ServerAction,
    },
    
    /// Pair with a sync server (cloud or local)
    Pair {
        /// Server URL (local server) or "cloud" for reMarkable cloud
        #[arg(short, long, default_value = "cloud")]
        server: String,
        
        /// Skip TLS certificate verification (for self-signed certs)
        #[arg(long, default_value = "false")]
        insecure: bool,
        
        /// Pairing code (if already have one; otherwise will prompt)
        #[arg(short, long)]
        code: Option<String>,
        
        /// Device name for registration
        #[arg(long)]
        device_name: Option<String>,
    },
    
    /// Configuration management
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Subcommand)]
enum SyncAction {
    /// List all documents in cloud
    List {
        /// Output format (table, json)
        #[arg(short, long, default_value = "table")]
        format: String,
    },
    
    /// Download documents from cloud
    Pull {
        /// Output directory
        #[arg(short, long, default_value = "./docs")]
        output: PathBuf,
        
        /// Document ID to pull (all if not specified)
        #[arg(long)]
        doc_id: Option<String>,
        
        /// Include PDF files
        #[arg(long, default_value = "true")]
        include_pdf: bool,
        
        /// Overwrite existing files
        #[arg(long, default_value = "false")]
        force: bool,
    },
    
    /// Upload documents to cloud
    Push {
        /// File or directory to upload
        #[arg(short, long)]
        input: PathBuf,
        
        /// Target folder ID (root if not specified)
        #[arg(long)]
        folder: Option<String>,
        
        /// Document name (derived from filename if not specified)
        #[arg(long)]
        name: Option<String>,
    },
    
    /// Show sync status
    Status,
    
    /// Show root hash and generation
    Root,
}

#[derive(Subcommand)]
enum BackupAction {
    /// Create a full cloud backup
    Create {
        /// Output directory (default: ~/.local/share/remarkable/backups/<timestamp>)
        #[arg(short, long)]
        output: Option<PathBuf>,
        
        /// Backup name/label
        #[arg(short, long)]
        name: Option<String>,
        
        /// Include PDFs
        #[arg(long, default_value = "true")]
        include_pdf: bool,
    },
    
    /// Restore from a backup
    Restore {
        /// Backup directory to restore from
        #[arg(short, long)]
        backup: PathBuf,
        
        /// Dry run (show what would be restored)
        #[arg(long, default_value = "false")]
        dry_run: bool,
        
        /// Document ID to restore (all if not specified)
        #[arg(long)]
        doc_id: Option<String>,
    },
    
    /// List local backups
    List,
    
    /// Show backup details
    Info {
        /// Backup directory
        #[arg(short, long)]
        backup: PathBuf,
    },
}

#[derive(Subcommand)]
enum DeviceAction {
    /// Show device information
    Info {
        /// Device address (default: 10.11.99.1 for USB, or hostname for SSH)
        #[arg(short, long)]
        address: Option<String>,
        
        /// Connection method
        #[arg(long, default_value = "usb")]
        method: String,
    },
    
    /// Capture device screenshot
    Screenshot {
        /// Output file
        #[arg(short, long)]
        output: PathBuf,
        
        /// Device address
        #[arg(short, long)]
        address: Option<String>,
        
        /// Copy the raw framebuffer over SSH instead of using screen share
        #[arg(long, default_value = "false")]
        ssh: bool,

        /// Take the screenshot through screen share, negotiated via this
        /// remarkable-server broker (e.g. remarkable.unwrap.rs). Screen share
        /// must be on on the tablet.
        #[arg(long)]
        cloud: Option<String>,

        /// Broker TLS port
        #[arg(long, default_value_t = 8883)]
        broker_port: u16,

        /// File holding a user token issued by the broker's server
        #[arg(long, env = "REMARKABLE_USER_TOKEN_FILE")]
        user_token: Option<PathBuf>,

        /// User id in the broker's signaling topics
        #[arg(long, default_value = "local-user")]
        user_id: String,
    },
    
    /// USB Web UI operations
    Usb {
        #[command(subcommand)]
        action: UsbAction,
    },
    
    /// SSH operations
    Ssh {
        #[command(subcommand)]
        action: SshAction,
    },
}

#[derive(Subcommand)]
enum UsbAction {
    /// List documents via USB
    List,
    
    /// Upload file via USB
    Upload {
        /// File to upload (PDF, EPUB)
        #[arg(short, long)]
        file: PathBuf,
    },
    
    /// Download document via USB
    Download {
        /// Document ID
        #[arg(short, long)]
        doc_id: String,
        
        /// Output file
        #[arg(short, long)]
        output: PathBuf,
    },
    
    /// Delete document via USB
    Delete {
        /// Document ID
        #[arg(short, long)]
        doc_id: String,
        
        /// Skip confirmation
        #[arg(long, default_value = "false")]
        force: bool,
    },
}

#[derive(Subcommand)]
enum SshAction {
    /// Execute SSH command on device
    Exec {
        /// SSH host (default: 10.11.99.1)
        #[arg(short, long, default_value = "10.11.99.1")]
        host: String,
        
        /// SSH user
        #[arg(short, long, default_value = "root")]
        user: String,
        
        /// Command to execute
        #[arg(trailing_var_arg = true)]
        command: Vec<String>,
    },
    
    /// Get device tokens via SSH
    GetTokens {
        /// SSH host
        #[arg(short, long, default_value = "10.11.99.1")]
        host: String,
        
        /// Output directory for tokens
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    
    /// Restart xochitl service
    RestartXochitl {
        /// SSH host
        #[arg(short, long, default_value = "10.11.99.1")]
        host: String,
    },
}

#[derive(Subcommand)]
enum MqttAction {
    /// Listen for real-time events
    Listen {
        /// Event types to show (all, sync, notification)
        #[arg(short, long, default_value = "all")]
        filter: String,
        
        /// Output format (text, json)
        #[arg(short, long, default_value = "text")]
        format: String,
        
        /// Exit after N events
        #[arg(long)]
        count: Option<usize>,
        
        /// Timeout in seconds
        #[arg(long)]
        timeout: Option<u64>,
    },
    
    /// Show MQTT connection status
    Status,
}

#[derive(Subcommand)]
enum ServerAction {
    /// Start local sync server
    Start {
        /// Port to listen on
        #[arg(short, long, default_value = "8080")]
        port: u16,
        
        /// Data directory
        #[arg(short, long)]
        data_dir: Option<PathBuf>,
        
        /// Enable CORS
        #[arg(long, default_value = "true")]
        cors: bool,
    },
    
    /// Stop running server
    Stop,
    
    /// Show server status
    Status,
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Initialize configuration
    Init {
        /// Overwrite existing config
        #[arg(long, default_value = "false")]
        force: bool,
    },
    
    /// Show current configuration
    Show,
    
    /// Set configuration value
    Set {
        /// Key to set
        #[arg(short, long)]
        key: String,
        
        /// Value to set
        #[arg(short, long)]
        value: String,
    },
    
    /// Import tokens from device
    ImportTokens {
        /// SSH host (default: 10.11.99.1)
        #[arg(short, long, default_value = "10.11.99.1")]
        host: String,
    },
}

/// Document info for table display
#[derive(Tabled)]
struct DocRow {
    #[tabled(rename = "Type")]
    doc_type: String,
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Hash")]
    hash: String,
}

/// Backup metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
struct BackupMetadata {
    created: String,
    name: Option<String>,
    document_count: usize,
    total_files: usize,
    total_size: u64,
    root_hash: String,
    generation: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    
    // Initialize tracing
    let filter = if cli.verbose {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("debug"))
    } else {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"))
    };
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .without_time()
        .init();
    
    // Resolve token paths
    let config = config_dir();
    let device_token_path = cli.device_token.unwrap_or_else(|| config.join("device_token.txt"));
    let user_token_path = cli.user_token.unwrap_or_else(|| config.join("user_token.txt"));
    
    match cli.command {
        Commands::Info { file } => cmd_info(&file).await?,
        Commands::Parse { file, svg } => cmd_parse(&file, svg.as_ref()).await?,
        Commands::AnnotatePdf { pdf, rm_dir, output } => {
            cmd_annotate_pdf(&pdf, &rm_dir, &output).await?
        }
        Commands::ExportAll { input, output, ignore_errors } => {
            cmd_export_all(&input, &output, ignore_errors).await?
        }
        Commands::Sync { action } => {
            let client = SyncClient::from_token_files(
                device_token_path.to_str().unwrap(),
                user_token_path.to_str().unwrap()
            )?;
            cmd_sync(action, client).await?
        }
        Commands::Backup { action } => {
            let client = SyncClient::from_token_files(
                device_token_path.to_str().unwrap(),
                user_token_path.to_str().unwrap()
            )?;
            cmd_backup(action, client).await?
        }
        Commands::Device { action } => cmd_device(action).await?,
        Commands::Mqtt { action } => {
            let device_token = load_token(&device_token_path)?;
            let user_token = load_token(&user_token_path)?;
            cmd_mqtt(action, &device_token, &user_token).await?
        }
        Commands::Server { action } => cmd_server(action).await?,
        Commands::Pair { server, insecure, code, device_name } => {
            cmd_pair(&server, insecure, code.as_deref(), device_name.as_deref(), &config).await?
        }
        Commands::Config { action } => cmd_config(action, &config).await?,
    }
    
    Ok(())
}

// ============ Info Command ============

async fn cmd_info(file: &PathBuf) -> Result<()> {
    let data = std::fs::read(file)?;
    let strokes = parse_rm_file(&data)?;
    
    println!("{}", "File Information".bold());
    println!("  Path: {}", file.display());
    println!("  Size: {} bytes", data.len());
    println!();
    
    println!("{}", "Stroke Statistics".bold());
    println!("  Total strokes: {}", strokes.len());
    
    let total_points: usize = strokes.iter().map(|s| s.points.len()).sum();
    println!("  Total points: {}", total_points);
    
    // Count pen types
    let mut pen_counts: HashMap<_, usize> = HashMap::new();
    for stroke in &strokes {
        *pen_counts.entry(stroke.pen).or_insert(0) += 1;
    }
    
    println!();
    println!("{}", "Pen Types".bold());
    for (pen, count) in pen_counts {
        println!("  {:?}: {}", pen, count);
    }
    
    // Count colors (using string keys since Color doesn't impl Hash)
    let mut color_counts: HashMap<String, usize> = HashMap::new();
    for stroke in &strokes {
        let key = format!("{:?}", stroke.color);
        *color_counts.entry(key).or_insert(0) += 1;
    }
    
    println!();
    println!("{}", "Colors".bold());
    for (color, count) in color_counts {
        println!("  {}: {}", color, count);
    }
    
    Ok(())
}

// ============ Parse Command ============

async fn cmd_parse(file: &PathBuf, svg: Option<&PathBuf>) -> Result<()> {
    let data = std::fs::read(file)?;
    let strokes = parse_rm_file(&data)?;
    
    println!("{} {} strokes", "Parsed".green(), strokes.len());
    
    if let Some(svg_path) = svg {
        let svg_content = strokes_to_svg(&strokes, 1872, 1404);
        std::fs::write(svg_path, svg_content)?;
        println!("{} SVG to {}", "Wrote".green(), svg_path.display());
    }
    
    Ok(())
}

// ============ Annotate PDF Command ============

async fn cmd_annotate_pdf(pdf: &PathBuf, rm_dir: &PathBuf, output: &PathBuf) -> Result<()> {
    let mut annotator = PdfAnnotator::open(pdf)?;
    
    println!("PDF has {} pages", annotator.page_count());
    
    let mut annotated = 0;
    for page in 0..annotator.page_count() {
        let patterns = vec![
            rm_dir.join(format!("{}.rm", page)),
            rm_dir.join(format!("page{}.rm", page)),
            rm_dir.join(format!("page_{}.rm", page)),
        ];
        
        for rm_path in patterns {
            if rm_path.exists() {
                match annotator.add_rm_file(page, &rm_path) {
                    Ok(()) => {
                        println!("{} page {} from {}", "Annotated".green(), page, rm_path.display());
                        annotated += 1;
                    }
                    Err(e) => {
                        eprintln!("{}: page {}: {}", "Failed".red(), page, e);
                    }
                }
                break;
            }
        }
    }
    
    annotator.save(output)?;
    println!("{} annotated PDF to {} ({} pages)", "Saved".green(), output.display(), annotated);
    
    Ok(())
}

// ============ Export All Command ============

async fn cmd_export_all(input: &PathBuf, output: &PathBuf, ignore_errors: bool) -> Result<()> {
    std::fs::create_dir_all(output)?;
    
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(ProgressStyle::default_spinner().template("{spinner:.green} {msg}").unwrap());
    spinner.set_message("Scanning for .rm files...");
    
    // Collect all .rm files
    let rm_files: Vec<_> = walkdir::WalkDir::new(input)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "rm"))
        .collect();
    
    spinner.finish_with_message(format!("Found {} .rm files", rm_files.len()));
    
    let progress = ProgressBar::new(rm_files.len() as u64);
    progress.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} ({eta})")
            .unwrap()
            .progress_chars("#>-")
    );
    
    let mut count = 0;
    let mut errors = 0;
    
    for entry in rm_files {
        let rm_path = entry.path();
        let relative = rm_path.strip_prefix(input).unwrap_or(rm_path);
        let svg_path = output.join(relative).with_extension("svg");
        
        if let Some(parent) = svg_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        
        match std::fs::read(rm_path) {
            Ok(data) => {
                match parse_rm_file(&data) {
                    Ok(strokes) => {
                        let svg = strokes_to_svg(&strokes, 1872, 1404);
                        std::fs::write(&svg_path, svg)?;
                        count += 1;
                    }
                    Err(e) => {
                        if !ignore_errors {
                            progress.finish_and_clear();
                            return Err(CliError::Lines(e));
                        }
                        debug!("Parse error for {}: {}", rm_path.display(), e);
                        errors += 1;
                    }
                }
            }
            Err(e) => {
                if !ignore_errors {
                    progress.finish_and_clear();
                    return Err(CliError::Io(e));
                }
                debug!("Read error for {}: {}", rm_path.display(), e);
                errors += 1;
            }
        }
        
        progress.inc(1);
    }
    
    progress.finish_and_clear();
    
    println!("\n{}", "Export Complete".bold().green());
    println!("  Exported: {}", count);
    if errors > 0 {
        println!("  Errors: {}", errors.to_string().red());
    }
    
    Ok(())
}

// ============ Sync Commands ============

async fn cmd_sync(action: SyncAction, client: SyncClient) -> Result<()> {
    match action {
        SyncAction::List { format } => {
            let docs = client.list_documents().await?;
            
            if format == "json" {
                println!("{}", serde_json::to_string_pretty(&docs)?);
            } else {
                let rows: Vec<DocRow> = docs.iter().map(|d| DocRow {
                    doc_type: if d.entry_type == "CollectionType" { "📁".into() } else { "📄".into() },
                    id: d.uuid.clone(),
                    hash: d.hash[..12].to_string() + "...",
                }).collect();
                
                println!("{}", "Documents".bold());
                println!("{}", Table::new(rows));
                println!("\nTotal: {} items", docs.len());
            }
        }
        
        SyncAction::Pull { output, doc_id, include_pdf, force } => {
            std::fs::create_dir_all(&output)?;
            
            if let Some(id) = doc_id {
                println!("Downloading document {}...", id);
                let doc = client.download_document(&id).await?;
                
                let doc_dir = output.join(&id);
                std::fs::create_dir_all(&doc_dir)?;
                
                for (page_id, data) in &doc.pages {
                    let file_path = doc_dir.join(format!("{}.rm", page_id));
                    if !force && file_path.exists() {
                        println!("  Skipping {} (exists)", page_id);
                        continue;
                    }
                    std::fs::write(&file_path, data)?;
                }
                
                if let Some(ref metadata) = doc.metadata {
                    let meta_json = serde_json::to_string_pretty(metadata)?;
                    std::fs::write(doc_dir.join("metadata.json"), meta_json)?;
                }
                
                if let Some(ref content) = doc.content {
                    let content_json = serde_json::to_string_pretty(content)?;
                    std::fs::write(doc_dir.join("content.json"), content_json)?;
                }
                
                if include_pdf {
                    if let Some(ref pdf_data) = doc.pdf {
                        std::fs::write(doc_dir.join("document.pdf"), pdf_data)?;
                    }
                }
                
                println!("{} {} pages to {}", "Downloaded".green(), doc.pages.len(), doc_dir.display());
            } else {
                println!("Downloading all documents...");
                let ids = client.download_all(output.to_str().unwrap()).await?;
                println!("{} {} documents", "Downloaded".green(), ids.len());
            }
        }
        
        SyncAction::Push { input, folder, name } => {
            // Note: Upload requires subscription, may fail with 400
            println!("{}: Upload requires reMarkable Connect subscription", "Note".yellow());
            println!("Attempting upload of {}...", input.display());
            
            if !input.exists() {
                return Err(CliError::Other(format!("File not found: {}", input.display())));
            }
            
            let data = std::fs::read(&input)?;
            let filename = name.unwrap_or_else(|| {
                input.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("document")
                    .to_string()
            });
            
            match client.upload_file(&data, &filename).await {
                Ok(result) => {
                    println!("{}: {:?}", "Uploaded".green(), result);
                }
                Err(e) => {
                    eprintln!("{}: {}", "Upload failed".red(), e);
                    eprintln!("This may require an active Connect subscription.");
                }
            }
        }
        
        SyncAction::Status => {
            let root = client.get_root().await?;
            let docs = client.list_documents().await?;
            
            println!("{}", "Sync Status".bold());
            println!("  Root hash: {}", root.hash[..16].to_string() + "...");
            println!("  Generation: {}", root.generation);
            println!("  Documents: {}", docs.len());
            
            let folders = docs.iter().filter(|d| d.entry_type == "CollectionType").count();
            let files = docs.len() - folders;
            println!("  Folders: {}", folders);
            println!("  Files: {}", files);
        }
        
        SyncAction::Root => {
            let root = client.get_root().await?;
            println!("Root hash: {}", root.hash);
            println!("Generation: {}", root.generation);
        }
    }
    
    Ok(())
}

// ============ Backup Commands ============

async fn cmd_backup(action: BackupAction, client: SyncClient) -> Result<()> {
    let backup_base = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("remarkable/backups");
    
    match action {
        BackupAction::Create { output, name, include_pdf } => {
            let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
            let backup_dir = output.unwrap_or_else(|| backup_base.join(&timestamp));
            std::fs::create_dir_all(&backup_dir)?;
            
            println!("{} to {}...", "Starting backup".bold(), backup_dir.display());
            
            let root = client.get_root().await?;
            let ids = client.download_all(backup_dir.to_str().unwrap()).await?;
            
            // Count files and calculate size
            let mut total_files = 0;
            let mut total_size = 0u64;
            for entry in walkdir::WalkDir::new(&backup_dir) {
                if let Ok(e) = entry {
                    if e.file_type().is_file() {
                        total_files += 1;
                        total_size += e.metadata().map(|m| m.len()).unwrap_or(0);
                    }
                }
            }
            
            // Save metadata
            let metadata = BackupMetadata {
                created: chrono::Local::now().to_rfc3339(),
                name,
                document_count: ids.len(),
                total_files,
                total_size,
                root_hash: root.hash,
                generation: root.generation,
            };
            
            let meta_json = serde_json::to_string_pretty(&metadata)?;
            std::fs::write(backup_dir.join("backup.json"), meta_json)?;
            
            println!("\n{}", "Backup Complete".bold().green());
            println!("  Documents: {}", ids.len());
            println!("  Files: {}", total_files);
            println!("  Size: {} MB", total_size / 1_000_000);
            println!("  Location: {}", backup_dir.display());
        }
        
        BackupAction::Restore { backup, dry_run, doc_id } => {
            if !backup.exists() {
                return Err(CliError::BackupNotFound(backup.display().to_string()));
            }
            
            let meta_path = backup.join("backup.json");
            let metadata: BackupMetadata = if meta_path.exists() {
                serde_json::from_str(&std::fs::read_to_string(&meta_path)?)?
            } else {
                return Err(CliError::Other("Invalid backup: missing backup.json".into()));
            };
            
            println!("{}", "Restore Preview".bold());
            println!("  Backup: {}", backup.display());
            println!("  Created: {}", metadata.created);
            println!("  Documents: {}", metadata.document_count);
            
            if dry_run {
                println!("\n{}: Dry run, no changes made", "Note".yellow());
                // List documents that would be restored
                for entry in std::fs::read_dir(&backup)? {
                    let entry = entry?;
                    if entry.file_type()?.is_dir() {
                        println!("  Would restore: {}", entry.file_name().to_string_lossy());
                    }
                }
            } else {
                println!("\n{}: Restore requires upload capability", "Warning".yellow());
                println!("Upload API requires Connect subscription.");
                // Actual restore would use upload_file for each document
            }
        }
        
        BackupAction::List => {
            if !backup_base.exists() {
                println!("No backups found");
                return Ok(());
            }
            
            println!("{}", "Local Backups".bold());
            
            for entry in std::fs::read_dir(&backup_base)? {
                let entry = entry?;
                let meta_path = entry.path().join("backup.json");
                
                if meta_path.exists() {
                    if let Ok(meta_str) = std::fs::read_to_string(&meta_path) {
                        if let Ok(meta) = serde_json::from_str::<BackupMetadata>(&meta_str) {
                            println!("  {} - {} docs, {} MB ({})",
                                entry.file_name().to_string_lossy(),
                                meta.document_count,
                                meta.total_size / 1_000_000,
                                meta.name.unwrap_or_else(|| "unnamed".into())
                            );
                        }
                    }
                }
            }
        }
        
        BackupAction::Info { backup } => {
            let meta_path = backup.join("backup.json");
            let metadata: BackupMetadata = serde_json::from_str(
                &std::fs::read_to_string(&meta_path)?
            )?;
            
            println!("{}", "Backup Details".bold());
            println!("  Created: {}", metadata.created);
            if let Some(name) = metadata.name {
                println!("  Name: {}", name);
            }
            println!("  Documents: {}", metadata.document_count);
            println!("  Total files: {}", metadata.total_files);
            println!("  Total size: {} MB", metadata.total_size / 1_000_000);
            println!("  Root hash: {}", metadata.root_hash[..16].to_string() + "...");
            println!("  Generation: {}", metadata.generation);
        }
    }
    
    Ok(())
}

// ============ Device Commands ============

async fn cmd_device(action: DeviceAction) -> Result<()> {
    match action {
        DeviceAction::Info { address, method } => {
            let addr = address.as_deref().unwrap_or("10.11.99.1");
            
            match method.as_str() {
                "usb" => {
                    let client = UsbClient::with_address(addr);
                    
                    if !client.is_connected().await {
                        return Err(CliError::DeviceNotConnected);
                    }
                    
                    let docs = client.list_documents().await?;
                    
                    println!("{}", "Device Info (USB)".bold());
                    println!("  Address: {}", addr);
                    println!("  Status: {}", "Connected".green());
                    println!("  Documents: {}", docs.len());
                }
                "ssh" => {
                    println!("{}", "Device Info (SSH)".bold());
                    println!("  Host: {}", addr);
                    
                    // Use ssh to get device info
                    let output = std::process::Command::new("ssh")
                        .args([
                            "-o", "ConnectTimeout=5",
                            &format!("root@{}", addr),
                            "cat /sys/devices/soc0/machine && cat /etc/version"
                        ])
                        .output();
                    
                    match output {
                        Ok(out) if out.status.success() => {
                            let info = String::from_utf8_lossy(&out.stdout);
                            for line in info.lines() {
                                println!("  {}", line);
                            }
                        }
                        _ => {
                            println!("  Status: {}", "Not reachable".red());
                        }
                    }
                }
                _ => {
                    return Err(CliError::Other(format!("Unknown method: {}", method)));
                }
            }
        }
        
        DeviceAction::Screenshot { output, address, ssh, cloud, broker_port, user_token, user_id } => {
            let addr = address.as_deref().unwrap_or("10.11.99.1");
            
            if let Some(host) = cloud {
                let token_path = user_token.ok_or_else(|| CliError::Other("--cloud needs --user-token <file>".into()))?;
                let user_token = std::fs::read_to_string(&token_path)?.trim().to_string();
                if user_token.is_empty() {
                    return Err(CliError::Other(format!("user token file {} is empty", token_path.display())));
                }
                println!("Capturing screenshot via screen share ({host})...");
                let frame = screen_share_frame(remarkable_screenshare::cloud::CloudConfig {
                    host,
                    port: broker_port,
                    user_token,
                    user_id,
                    transport: Default::default(),
                    timeout: std::time::Duration::from_secs(30),
                }).await?;
                write_png(&output, &frame)?;
                println!("{} {}x{} screenshot to {}", "Saved".green(), frame.width, frame.height, output.display());
            } else if ssh {
                // SSH method: grab framebuffer directly
                println!("Capturing screenshot via SSH...");
                
                let status = std::process::Command::new("ssh")
                    .args([
                        &format!("root@{}", addr),
                        "cat /dev/fb0 > /tmp/fb.raw && gzip -f /tmp/fb.raw"
                    ])
                    .status()?;
                
                if !status.success() {
                    return Err(CliError::Other("Failed to capture framebuffer".into()));
                }
                
                // Download the file
                let status = std::process::Command::new("scp")
                    .args([
                        &format!("root@{}:/tmp/fb.raw.gz", addr),
                        output.with_extension("raw.gz").to_str().unwrap()
                    ])
                    .status()?;
                
                if status.success() {
                    println!("{} screenshot to {}", "Saved".green(), output.display());
                    println!("Note: Raw framebuffer format, requires conversion");
                }
            } else {
                return Err(CliError::Other(
                    "choose --cloud <broker> (screen share) or --ssh (raw framebuffer)".into(),
                ));
            }
        }
        
        DeviceAction::Usb { action } => {
            let client = UsbClient::new();
            
            if !client.is_connected().await {
                return Err(CliError::DeviceNotConnected);
            }
            
            match action {
                UsbAction::List => {
                    let docs = client.list_documents().await?;
                    
                    println!("{}", "USB Documents".bold());
                    for doc in &docs {
                        let icon = if doc.doc_type == "CollectionType" { "📁" } else { "📄" };
                        println!("  {} {} ({})", icon, doc.visible_name, doc.id);
                    }
                    println!("\nTotal: {} items", docs.len());
                }
                
                UsbAction::Upload { file } => {
                    client.upload_file(&file).await?;
                    println!("{} {}", "Uploaded".green(), file.display());
                }
                
                UsbAction::Download { doc_id, output } => {
                    client.download_to_file(&doc_id, &output).await?;
                    println!("{} {} to {}", "Downloaded".green(), doc_id, output.display());
                }
                
                UsbAction::Delete { doc_id, force } => {
                    if !force {
                        println!("Delete document {}? [y/N] ", doc_id);
                        let mut input = String::new();
                        std::io::stdin().read_line(&mut input)?;
                        if !input.trim().eq_ignore_ascii_case("y") {
                            println!("Cancelled");
                            return Ok(());
                        }
                    }
                    
                    client.delete(&doc_id).await?;
                    println!("{} {}", "Deleted".green(), doc_id);
                }
            }
        }
        
        DeviceAction::Ssh { action } => {
            match action {
                SshAction::Exec { host, user, command } => {
                    let cmd_str = command.join(" ");
                    if cmd_str.is_empty() {
                        return Err(CliError::Other("No command specified".into()));
                    }
                    
                    let status = std::process::Command::new("ssh")
                        .args([&format!("{}@{}", user, host), &cmd_str])
                        .status()?;
                    
                    if !status.success() {
                        return Err(CliError::Other("SSH command failed".into()));
                    }
                }
                
                SshAction::GetTokens { host, output } => {
                    let output_dir = output.unwrap_or_else(config_dir);
                    std::fs::create_dir_all(&output_dir)?;
                    
                    // Get device token
                    let output_result = std::process::Command::new("ssh")
                        .args([
                            &format!("root@{}", host),
                            "grep devicetoken /home/root/.config/remarkable/xochitl.conf | cut -d= -f2"
                        ])
                        .output()?;
                    
                    if output_result.status.success() {
                        let token = String::from_utf8_lossy(&output_result.stdout);
                        let token = token.trim();
                        if !token.is_empty() {
                            let path = output_dir.join("device_token.txt");
                            std::fs::write(&path, token)?;
                            println!("{} device token to {}", "Saved".green(), path.display());
                        }
                    }
                    
                    println!("{}: User token must be obtained via authentication", "Note".yellow());
                }
                
                SshAction::RestartXochitl { host } => {
                    println!("Restarting xochitl on {}...", host);
                    
                    let status = std::process::Command::new("ssh")
                        .args([
                            &format!("root@{}", host),
                            "systemctl restart xochitl"
                        ])
                        .status()?;
                    
                    if status.success() {
                        println!("{} xochitl", "Restarted".green());
                    } else {
                        return Err(CliError::Other("Failed to restart xochitl".into()));
                    }
                }
            }
        }
    }
    
    Ok(())
}

// ============ MQTT Commands ============

async fn cmd_mqtt(action: MqttAction, device_token: &str, user_token: &str) -> Result<()> {
    match action {
        MqttAction::Listen { filter, format, count, timeout } => {
            let config = MqttConfig::from_tokens(device_token, user_token)?;
            
            println!("{}", "Connecting to MQTT broker...".bold());
            println!("  Broker: {}", config.broker);
            println!("  User: {}", config.user_id);
            
            let mut client = MqttClient::new(config);
            client.connect().await?;
            client.subscribe_default().await?;
            
            println!("{}", "Listening for events...".green());
            println!("Press Ctrl+C to stop\n");
            
            let mut event_count = 0;
            let timeout_duration = timeout.map(Duration::from_secs);
            
            loop {
                let poll_future = client.poll();
                
                let event = if let Some(timeout_dur) = timeout_duration {
                    match tokio::time::timeout(timeout_dur, poll_future).await {
                        Ok(result) => result?,
                        Err(_) => {
                            println!("Timeout reached");
                            break;
                        }
                    }
                } else {
                    poll_future.await?
                };
                
                // Filter events
                let should_show = match &event {
                    MqttEvent::SyncComplete { .. } => filter == "all" || filter == "sync",
                    MqttEvent::Notification { .. } => filter == "all" || filter == "notification",
                    MqttEvent::Connected | MqttEvent::Disconnected | MqttEvent::Ping => filter == "all",
                    _ => filter == "all",
                };
                
                if should_show {
                    if format == "json" {
                        println!("{}", serde_json::to_string(&format!("{:?}", event))?);
                    } else {
                        match &event {
                            MqttEvent::SyncComplete { sync, topic } => {
                                println!("[{}] {} generation {} from {}",
                                    "SYNC".cyan(),
                                    topic,
                                    sync.generation,
                                    sync.source_device_id
                                );
                            }
                            MqttEvent::Notification { notification, topic } => {
                                println!("[{}] {} - {}",
                                    "NOTIF".yellow(),
                                    topic,
                                    notification.message
                                );
                            }
                            MqttEvent::Connected => {
                                println!("[{}] Connected", "INFO".green());
                            }
                            MqttEvent::Disconnected => {
                                println!("[{}] Disconnected", "INFO".red());
                                break;
                            }
                            MqttEvent::Ping => {
                                debug!("Ping");
                            }
                            _ => {
                                debug!("Other event: {:?}", event);
                            }
                        }
                    }
                    
                    event_count += 1;
                    if let Some(max) = count {
                        if event_count >= max {
                            println!("\nReached event count limit ({})", max);
                            break;
                        }
                    }
                }
            }
            
            client.disconnect().await?;
        }
        
        MqttAction::Status => {
            let config = MqttConfig::from_tokens(device_token, user_token)?;
            
            println!("{}", "MQTT Configuration".bold());
            println!("  Broker: {}", config.broker);
            println!("  Port: {}", config.port);
            println!("  User ID: {}", config.user_id);
            
            // Try to connect
            println!("\nTesting connection...");
            let mut client = MqttClient::new(config);
            
            match client.connect().await {
                Ok(()) => {
                    println!("  Status: {}", "Connected".green());
                    client.disconnect().await?;
                }
                Err(e) => {
                    println!("  Status: {}", "Failed".red());
                    println!("  Error: {}", e);
                }
            }
        }
    }
    
    Ok(())
}

// ============ Server Commands ============

async fn cmd_server(action: ServerAction) -> Result<()> {
    match action {
        ServerAction::Start { port, data_dir, cors } => {
            println!("{}", "Local Sync Server".bold());
            println!("  Port: {}", port);
            println!("  Data: {:?}", data_dir);
            println!("  CORS: {}", cors);
            
            // Server implementation would go here
            // Using axum or warp to serve the sync API locally
            println!("\n{}: Server not yet implemented", "Note".yellow());
            println!("This would serve:");
            println!("  GET  /sync/v3/root");
            println!("  GET  /sync/v3/files/:hash");
            println!("  PUT  /sync/v3/files/:hash");
        }
        
        ServerAction::Stop => {
            println!("Stopping server...");
            // Would send signal to running server
        }
        
        ServerAction::Status => {
            println!("Server status: {}", "Not running".yellow());
        }
    }
    
    Ok(())
}

// ============ Pair Command ============

async fn cmd_pair(
    server: &str,
    insecure: bool,
    code: Option<&str>,
    device_name: Option<&str>,
    config_dir: &PathBuf,
) -> Result<()> {
    use remarkable_sync::local::{LocalServerClient, LocalServerConfig, ServerType};
    use std::io::{self, Write};
    
    println!("{}", "Device Pairing".bold());
    println!();
    
    let is_cloud = server == "cloud" || server.contains("remarkable.com") || server.contains("remarkable.engineering");
    
    if is_cloud {
        println!("  Server: {} (reMarkable Cloud)", "cloud".cyan());
        println!();
        println!("  To pair with reMarkable Cloud:");
        println!("  1. Go to https://my.remarkable.com/device/connect");
        println!("  2. Log in and get a pairing code");
        println!("  3. Enter the code below");
        println!();
        
        // Get code
        let pairing_code = if let Some(c) = code {
            c.to_string()
        } else {
            print!("  Enter pairing code: ");
            io::stdout().flush()?;
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            input.trim().to_string()
        };
        
        if pairing_code.len() != 8 {
            return Err(CliError::Other("Pairing code must be 8 characters".into()));
        }
        
        println!();
        println!("  {} Pairing with cloud...", "⏳".yellow());
        
        // Exchange code for tokens
        let client = reqwest::Client::new();
        let device_id = uuid::Uuid::new_v4().to_string();
        
        let device_token = remarkable_sync::client::auth::pair_device(&client, &pairing_code, &device_id)
            .await
            .map_err(|e| CliError::Other(format!("Pairing failed: {}", e)))?;
        
        let user_token = remarkable_sync::client::auth::refresh_user_token(&client, &device_token)
            .await
            .map_err(|e| CliError::Other(format!("Token refresh failed: {}", e)))?;
        
        // Save tokens
        std::fs::create_dir_all(config_dir)?;
        let device_path = config_dir.join("device_token.txt");
        let user_path = config_dir.join("user_token.txt");
        
        std::fs::write(&device_path, &device_token.token)?;
        std::fs::write(&user_path, &user_token.token)?;
        
        println!("  {} Pairing successful!", "✓".green());
        println!();
        println!("  Device token saved to: {}", device_path.display());
        println!("  User token saved to: {}", user_path.display());
        println!("  Region: {}", user_token.region.cyan());
        println!("  Scopes: {}", user_token.scopes.join(", ").dimmed());
        
    } else {
        println!("  Server: {} (Local)", server.cyan());
        
        // Create local server config
        let mut config = LocalServerConfig::new(server);
        if insecure {
            config = config.with_skip_tls_verify(true);
            println!("  {} TLS verification disabled", "⚠".yellow());
        }
        if let Some(name) = device_name {
            config = config.with_device_name(name);
        }
        
        // Detect server type
        println!();
        print!("  Detecting server type... ");
        io::stdout().flush()?;
        
        let local_client = LocalServerClient::new(config.clone())
            .map_err(|e| CliError::Other(format!("Failed to create client: {}", e)))?;
        
        match local_client.detect_server_type().await {
            Ok(ServerType::Local(version)) => {
                println!("{}", format!("remarkable-server v{}", version).green());
            }
            Ok(ServerType::Cloud) => {
                println!("{}", "Detected reMarkable Cloud".yellow());
                println!("  Use --server cloud for cloud pairing");
                return Ok(());
            }
            Ok(ServerType::Unknown) => {
                println!("{}", "Unknown server type (proceeding anyway)".yellow());
            }
            Err(e) => {
                println!("{}", format!("Detection failed: {}", e).red());
                println!("  Proceeding with local server pairing...");
            }
        }
        println!();
        
        // Get code
        let pairing_code = if let Some(c) = code {
            c.to_string()
        } else {
            // Request a pairing code from the server
            println!("  Requesting pairing code from server...");
            
            match local_client.request_pairing_code().await {
                Ok(resp) => {
                    println!();
                    println!("  {} Your pairing code is: {}", "→".cyan(), resp.code.bold().green());
                    if resp.expires_in > 0 {
                        println!("    (expires in {} seconds)", resp.expires_in);
                    }
                    println!();
                    println!("  Enter this code on your device to pair.");
                    println!("  Or press Enter to continue pairing with this code...");
                    
                    let mut input = String::new();
                    io::stdin().read_line(&mut input)?;
                    
                    resp.code
                }
                Err(e) => {
                    // Code request not supported, ask for manual entry
                    println!("  {} Could not request code: {}", "!".yellow(), e);
                    println!();
                    print!("  Enter pairing code: ");
                    io::stdout().flush()?;
                    let mut input = String::new();
                    io::stdin().read_line(&mut input)?;
                    input.trim().to_string()
                }
            }
        };
        
        if pairing_code.is_empty() {
            return Err(CliError::Other("Pairing code cannot be empty".into()));
        }
        
        println!();
        println!("  {} Exchanging code for tokens...", "⏳".yellow());
        
        // Exchange code
        let device_id = uuid::Uuid::new_v4().to_string();
        let tokens = local_client.exchange_code(&pairing_code, &device_id)
            .await
            .map_err(|e| CliError::Other(format!("Pairing failed: {}", e)))?;
        
        // Save tokens
        std::fs::create_dir_all(config_dir)?;
        let device_path = config_dir.join("device_token.txt");
        let user_path = config_dir.join("user_token.txt");
        let server_path = config_dir.join("server.txt");
        
        std::fs::write(&device_path, &tokens.device_token)?;
        std::fs::write(&user_path, &tokens.user_token)?;
        std::fs::write(&server_path, &tokens.server_url)?;
        
        println!("  {} Pairing successful!", "✓".green());
        println!();
        println!("  Device token saved to: {}", device_path.display());
        println!("  User token saved to: {}", user_path.display());
        println!("  Server URL saved to: {}", server_path.display());
    }
    
    println!();
    println!("  Run `{} sync list` to verify sync access.", "remarkable".cyan());
    
    Ok(())
}

// ============ Config Commands ============

async fn cmd_config(action: ConfigAction, config_dir: &PathBuf) -> Result<()> {
    match action {
        ConfigAction::Init { force } => {
            if config_dir.exists() && !force {
                println!("Config directory already exists: {}", config_dir.display());
                println!("Use --force to overwrite");
                return Ok(());
            }
            
            std::fs::create_dir_all(config_dir)?;
            
            // Create placeholder files
            let device_token_path = config_dir.join("device_token.txt");
            let user_token_path = config_dir.join("user_token.txt");
            
            if !device_token_path.exists() || force {
                std::fs::write(&device_token_path, "# Device token goes here\n")?;
            }
            if !user_token_path.exists() || force {
                std::fs::write(&user_token_path, "# User token goes here\n")?;
            }
            
            println!("{} config at {}", "Initialized".green(), config_dir.display());
            println!("\nNext steps:");
            println!("1. Connect device via USB and get tokens:");
            println!("   remarkable device ssh get-tokens");
            println!("2. Or authenticate via browser");
        }
        
        ConfigAction::Show => {
            println!("{}", "Configuration".bold());
            println!("  Config dir: {}", config_dir.display());
            
            let device_token_path = config_dir.join("device_token.txt");
            let user_token_path = config_dir.join("user_token.txt");
            
            println!("  Device token: {}", 
                if device_token_path.exists() { "✓ found".green().to_string() } 
                else { "✗ missing".red().to_string() }
            );
            println!("  User token: {}",
                if user_token_path.exists() { "✓ found".green().to_string() }
                else { "✗ missing".red().to_string() }
            );
        }
        
        ConfigAction::Set { key, value } => {
            println!("Setting {} = {}", key, value);
            // Would update config file
        }
        
        ConfigAction::ImportTokens { host } => {
            println!("Importing tokens from {}...", host);
            
            // Same as SshAction::GetTokens
            let output_result = std::process::Command::new("ssh")
                .args([
                    "-o", "ConnectTimeout=5",
                    &format!("root@{}", host),
                    "grep devicetoken /home/root/.config/remarkable/xochitl.conf | cut -d= -f2"
                ])
                .output()?;
            
            if output_result.status.success() {
                let token = String::from_utf8_lossy(&output_result.stdout);
                let token = token.trim();
                if !token.is_empty() {
                    std::fs::create_dir_all(config_dir)?;
                    let path = config_dir.join("device_token.txt");
                    std::fs::write(&path, token)?;
                    println!("{} device token", "Imported".green());
                } else {
                    println!("{}: No device token found in xochitl.conf", "Warning".yellow());
                }
            } else {
                return Err(CliError::DeviceNotConnected);
            }
            
            println!("{}: User token must be obtained via authentication", "Note".yellow());
        }
    }
    
    Ok(())
}

/// Join the tablet's screen share and return its first frame.
async fn screen_share_frame(
    cfg: remarkable_screenshare::cloud::CloudConfig,
) -> Result<remarkable_screenshare::Frame> {
    let remarkable_screenshare::cloud::CloudSession { webrtc, mut data_rx, signaling_task } =
        remarkable_screenshare::cloud::connect(cfg).await?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut tx = Some(tx);
    let pump = remarkable_screenshare::pump_frames(&mut data_rx, |update| {
        if let remarkable_screenshare::Update::Frame(frame) = update {
            if let Some(tx) = tx.take() {
                let _ = tx.send(frame);
            }
        }
    });
    let frame = tokio::select! {
        frame = rx => Ok(frame.ok()),
        ended = pump => ended.map(|()| None),
        _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => Ok(None),
    };
    // Clean up before reporting a stream error.
    signaling_task.abort();
    let _ = webrtc.close().await;
    frame?.ok_or_else(|| CliError::Other("tablet sent no frame".into()))
}

fn write_png(path: &std::path::Path, frame: &remarkable_screenshare::Frame) -> Result<()> {
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut encoder = png::Encoder::new(file, frame.width, frame.height);
    encoder.set_color(match frame.format {
        remarkable_screenshare::PixelFormat::Gray8 => png::ColorType::Grayscale,
        remarkable_screenshare::PixelFormat::Rgb8 => png::ColorType::Rgb,
    });
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .and_then(|mut w| w.write_image_data(&frame.data))
        .map_err(|e| CliError::Other(format!("PNG encode failed: {e}")))
}
