//! reMarkable CLI tool
//!
//! Commands:
//! - info: Display .rm file info
//! - parse: Parse .rm file and optionally export to SVG
//! - export-all: Export all .rm files in a directory to SVG
//! - sync: Sync operations (pull, push, list)
//! - backup: Backup all documents

use std::path::PathBuf;
use clap::{Parser, Subcommand};
use remarkable_lines::{parse_rm_file, strokes_to_svg};
use remarkable_sync::SyncClient;
use remarkable_pdf::PdfAnnotator;

#[derive(Parser)]
#[command(name = "remarkable")]
#[command(about = "reMarkable tablet tools", long_about = None)]
struct Cli {
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
    
    /// Parse .rm file and optionally export
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
    },
    
    /// Sync operations
    Sync {
        #[command(subcommand)]
        action: SyncAction,
    },
    
    /// Backup all documents
    Backup {
        /// Device token file
        #[arg(short, long, default_value = "device_token.txt")]
        device_token: PathBuf,
        
        /// User token file
        #[arg(short, long, default_value = "user_token.txt")]
        user_token: PathBuf,
        
        /// Output directory
        #[arg(short, long, default_value = "./backup")]
        output: PathBuf,
    },
}

#[derive(Subcommand)]
enum SyncAction {
    /// List all documents
    List {
        /// Device token file
        #[arg(short, long, default_value = "device_token.txt")]
        device_token: PathBuf,
        
        /// User token file
        #[arg(short, long, default_value = "user_token.txt")]
        user_token: PathBuf,
    },
    
    /// Pull documents from cloud
    Pull {
        /// Device token file
        #[arg(short, long, default_value = "device_token.txt")]
        device_token: PathBuf,
        
        /// User token file
        #[arg(short, long, default_value = "user_token.txt")]
        user_token: PathBuf,
        
        /// Output directory
        #[arg(short, long, default_value = "./docs")]
        output: PathBuf,
        
        /// Document ID to pull (all if not specified)
        #[arg(long)]
        doc_id: Option<String>,
    },
    
    /// Show sync root hash
    Root {
        /// Device token file
        #[arg(short, long, default_value = "device_token.txt")]
        device_token: PathBuf,
        
        /// User token file
        #[arg(short, long, default_value = "user_token.txt")]
        user_token: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    
    match cli.command {
        Commands::Info { file } => {
            let data = std::fs::read(&file)?;
            let strokes = parse_rm_file(&data)?;
            
            println!("File: {}", file.display());
            println!("Strokes: {}", strokes.len());
            
            let total_points: usize = strokes.iter().map(|s| s.points.len()).sum();
            println!("Total points: {}", total_points);
            
            // Count pen types
            let mut pen_counts = std::collections::HashMap::new();
            for stroke in &strokes {
                *pen_counts.entry(stroke.pen).or_insert(0) += 1;
            }
            
            println!("\nPen types:");
            for (pen, count) in pen_counts {
                println!("  {:?}: {}", pen, count);
            }
        }
        
        Commands::Parse { file, svg } => {
            let data = std::fs::read(&file)?;
            let strokes = parse_rm_file(&data)?;
            
            println!("Parsed {} strokes", strokes.len());
            
            if let Some(svg_path) = svg {
                let svg_content = strokes_to_svg(&strokes, 1872, 1404);
                std::fs::write(&svg_path, svg_content)?;
                println!("Wrote SVG to {}", svg_path.display());
            }
        }
        
        Commands::AnnotatePdf { pdf, rm_dir, output } => {
            let mut annotator = PdfAnnotator::open(&pdf)?;
            
            println!("PDF has {} pages", annotator.page_count());
            
            // Look for .rm files matching page numbers
            let mut annotated = 0;
            for page in 0..annotator.page_count() {
                // Try various naming patterns
                let patterns = vec![
                    rm_dir.join(format!("{}.rm", page)),
                    rm_dir.join(format!("page{}.rm", page)),
                    rm_dir.join(format!("page_{}.rm", page)),
                ];
                
                for rm_path in patterns {
                    if rm_path.exists() {
                        match annotator.add_rm_file(page, &rm_path) {
                            Ok(()) => {
                                println!("Added annotations to page {} from {}", page, rm_path.display());
                                annotated += 1;
                            }
                            Err(e) => {
                                eprintln!("Failed to add {} to page {}: {}", rm_path.display(), page, e);
                            }
                        }
                        break;
                    }
                }
            }
            
            annotator.save(&output)?;
            println!("Saved annotated PDF to {} ({} pages annotated)", output.display(), annotated);
        }
        
        Commands::ExportAll { input, output } => {
            // Create output directory
            std::fs::create_dir_all(&output)?;
            
            // Find all .rm files recursively
            let mut count = 0;
            let mut errors = 0;
            
            for entry in walkdir::WalkDir::new(&input)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().map_or(false, |ext| ext == "rm"))
            {
                let rm_path = entry.path();
                let relative = rm_path.strip_prefix(&input).unwrap_or(rm_path);
                let svg_path = output.join(relative).with_extension("svg");
                
                // Create parent directories
                if let Some(parent) = svg_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                
                // Parse and export
                match std::fs::read(rm_path) {
                    Ok(data) => {
                        match parse_rm_file(&data) {
                            Ok(strokes) => {
                                let svg = strokes_to_svg(&strokes, 1872, 1404);
                                std::fs::write(&svg_path, svg)?;
                                count += 1;
                                println!("Exported: {}", svg_path.display());
                            }
                            Err(e) => {
                                eprintln!("Failed to parse {}: {}", rm_path.display(), e);
                                errors += 1;
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to read {}: {}", rm_path.display(), e);
                        errors += 1;
                    }
                }
            }
            
            println!("\nExported {} files ({} errors)", count, errors);
        }
        
        Commands::Sync { action } => {
            match action {
                SyncAction::List { device_token, user_token } => {
                    let client = SyncClient::from_token_files(
                        device_token.to_str().unwrap(),
                        user_token.to_str().unwrap()
                    )?;
                    
                    let docs = client.list_documents().await?;
                    
                    println!("Documents ({}):", docs.len());
                    for doc in docs {
                        let doc_type = if doc.entry_type == "CollectionType" {
                            "folder"
                        } else {
                            "doc"
                        };
                        println!("  [{}] {} ({})", doc_type, doc.uuid, doc.hash);
                    }
                }
                
                SyncAction::Pull { device_token, user_token, output, doc_id } => {
                    let client = SyncClient::from_token_files(
                        device_token.to_str().unwrap(),
                        user_token.to_str().unwrap()
                    )?;
                    
                    std::fs::create_dir_all(&output)?;
                    
                    if let Some(id) = doc_id {
                        println!("Downloading document {}...", id);
                        let doc = client.download_document(&id).await?;
                        
                        // Save files
                        let doc_dir = output.join(&id);
                        std::fs::create_dir_all(&doc_dir)?;
                        
                        // Save pages
                        for (page_id, data) in &doc.pages {
                            let file_path = doc_dir.join(format!("{}.rm", page_id));
                            std::fs::write(&file_path, data)?;
                        }
                        
                        // Save metadata if available
                        if let Some(ref metadata) = doc.metadata {
                            let meta_json = serde_json::to_string_pretty(metadata)?;
                            std::fs::write(doc_dir.join("metadata.json"), meta_json)?;
                        }
                        
                        // Save content if available
                        if let Some(ref content) = doc.content {
                            let content_json = serde_json::to_string_pretty(content)?;
                            std::fs::write(doc_dir.join("content.json"), content_json)?;
                        }
                        
                        println!("Downloaded {} pages", doc.pages.len());
                    } else {
                        println!("Downloading all documents...");
                        let ids = client.download_all(output.to_str().unwrap()).await?;
                        println!("Downloaded {} documents", ids.len());
                    }
                }
                
                SyncAction::Root { device_token, user_token } => {
                    let client = SyncClient::from_token_files(
                        device_token.to_str().unwrap(),
                        user_token.to_str().unwrap()
                    )?;
                    
                    let root = client.get_root().await?;
                    println!("Root hash: {}", root.hash);
                    println!("Generation: {}", root.generation);
                }
            }
        }
        
        Commands::Backup { device_token, user_token, output } => {
            let client = SyncClient::from_token_files(
                device_token.to_str().unwrap(),
                user_token.to_str().unwrap()
            )?;
            
            std::fs::create_dir_all(&output)?;
            
            println!("Starting backup to {}...", output.display());
            let ids = client.download_all(output.to_str().unwrap()).await?;
            println!("Backed up {} documents", ids.len());
        }
    }
    
    Ok(())
}
