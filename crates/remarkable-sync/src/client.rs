//! Sync client for reMarkable cloud
//!
//! Implements the sync protocol for downloading and uploading documents.
//!
//! The sync protocol uses:
//! - Merkle tree hash structure for efficient delta sync
//! - rm-filename header for file identification  
//! - SHA-256 content hashes for deduplication
//! - CRC32C checksums for upload verification (Google Cloud Storage)
//!
//! ## Hash Tree Structure
//! 
//! ```text
//! Root (from /sync/v3/root)
//!   └── Root index (documents.json format)
//!         └── Document hashes
//!               └── Document index (schema.txt format)
//!                     └── File hashes (.content, .metadata, .rm files)
//! ```

use reqwest::{Client, header};
use crate::SyncError;
use crate::checksum::x_goog_hash_header;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Tectonic regions for geo-routing
pub const TECTONIC_REGIONS: &[&str] = &[
    "eu",    // Europe (default)
    "us",    // United States  
    "asia",  // Asia
    "aus",   // Australia
];

/// Base URL for sync API (region is substituted)
pub const SYNC_API_BASE: &str = "https://{region}.tectonic.remarkable.com";

/// Auth API base
pub const AUTH_API_BASE: &str = "https://webapp.cloud.remarkable.engineering";

/// Discovery URL
pub const DISCOVERY_URL: &str = "https://internal.cloud.remarkable.com";

/// Device token (long-lived)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceToken {
    pub token: String,
}

/// User token (short-lived, ~3 hours)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserToken {
    pub token: String,
    pub region: String,
    pub scopes: Vec<String>,
}

/// Sync root response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRoot {
    #[serde(rename = "hash")]
    pub hash: String,
    #[serde(rename = "generation")]
    pub generation: u64,
    #[serde(rename = "schemaVersion", default)]
    pub schema_version: u32,
}

/// Document entry from root index
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocEntry {
    pub hash: String,
    #[serde(rename = "type")]
    pub entry_type: String,
    pub uuid: String,
    pub version: String,
    pub size: String,
}

/// File entry from document schema
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub hash: String,
    pub filename: String,
    pub size: u64,
}

/// Document schema (parsed from schema.txt)
#[derive(Debug, Clone)]
pub struct DocumentSchema {
    pub files: Vec<FileEntry>,
}

impl DocumentSchema {
    /// Parse schema.txt format
    /// 
    /// Format: 
    /// ```text
    /// <count>
    /// <hash>:<flags>:<filename>:<offset>:<size>
    /// ```
    pub fn parse(data: &str) -> Result<Self, SyncError> {
        let lines: Vec<&str> = data.lines().collect();
        if lines.is_empty() {
            return Err(SyncError::Parse("Empty schema".into()));
        }
        
        let count: usize = lines[0].trim().parse()
            .map_err(|_| SyncError::Parse("Invalid schema count".into()))?;
        
        let mut files = Vec::with_capacity(count);
        for line in lines.iter().skip(1) {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 5 {
                files.push(FileEntry {
                    hash: parts[0].to_string(),
                    filename: parts[2].to_string(),
                    size: parts[4].parse().unwrap_or(0),
                });
            }
        }
        
        Ok(Self { files })
    }
    
    /// Serialize schema back to text format
    pub fn serialize(&self) -> String {
        let mut output = format!("{}\n", self.files.len());
        for file in &self.files {
            // Format: hash:flags:filename:offset:size
            output.push_str(&format!(
                "{}:0:{}:0:{}\n",
                file.hash, file.filename, file.size
            ));
        }
        output
    }
}

/// Document metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocMetadata {
    #[serde(rename = "createdTime")]
    pub created_time: Option<String>,
    #[serde(rename = "lastModified")]
    pub last_modified: Option<String>,
    #[serde(rename = "lastOpened")]
    pub last_opened: Option<String>,
    #[serde(rename = "lastOpenedPage")]
    pub last_opened_page: Option<u32>,
    pub parent: Option<String>,
    pub pinned: Option<bool>,
    #[serde(rename = "type")]
    pub doc_type: Option<String>,
    #[serde(rename = "visibleName")]
    pub visible_name: Option<String>,
    /// Delete flag (soft delete)
    #[serde(default)]
    pub deleted: Option<bool>,
}

impl DocMetadata {
    /// Check if this is a folder
    pub fn is_folder(&self) -> bool {
        self.doc_type.as_ref().map_or(false, |t| t == "CollectionType")
    }
    
    /// Check if this is a document
    pub fn is_document(&self) -> bool {
        self.doc_type.as_ref().map_or(false, |t| t == "DocumentType")
    }
}

/// Content file structure (pages and transforms)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentFile {
    #[serde(rename = "coverPageNumber")]
    pub cover_page_number: Option<i32>,
    #[serde(rename = "fileType")]
    pub file_type: Option<String>,
    #[serde(rename = "pageCount")]
    pub page_count: Option<u32>,
    #[serde(rename = "cPages")]
    pub c_pages: Option<CPages>,
}

/// C-pages (CRDT page metadata)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CPages {
    #[serde(rename = "pages")]
    pub pages: Option<Vec<CPageInfo>>,
}

/// C-page info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CPageInfo {
    pub id: String,
}

/// A fully downloaded document with all its files
#[derive(Debug, Clone)]
pub struct DownloadedDocument {
    pub id: String,
    pub metadata: Option<DocMetadata>,
    pub content: Option<ContentFile>,
    pub pages: HashMap<String, Vec<u8>>,  // page_id -> .rm file content
    pub pdf: Option<Vec<u8>>,
}

/// Upload context for batch operations
#[derive(Debug, Clone)]
pub struct UploadContext {
    /// Unique sync session ID
    pub sync_id: String,
    /// Current batch number (increments for each file in batch)
    pub batch_number: u32,
    /// Parent hash (root hash for top-level ops)
    pub parent_hash: String,
    /// Expected generation for optimistic locking
    pub expect_generation: Option<u64>,
}

impl UploadContext {
    /// Create a new upload context for a sync session
    pub fn new(parent_hash: String) -> Self {
        Self {
            sync_id: Uuid::new_v4().to_string(),
            batch_number: 0,
            parent_hash,
            expect_generation: None,
        }
    }
    
    /// Increment batch number and return current
    pub fn next_batch(&mut self) -> u32 {
        let current = self.batch_number;
        self.batch_number += 1;
        current
    }
}

/// Upload result with hash information
#[derive(Debug, Clone)]
pub struct UploadResult {
    /// SHA-256 hash of uploaded content
    pub hash: String,
    /// Size in bytes
    pub size: u64,
}

/// Server mode for sync client
#[derive(Debug, Clone, Default)]
pub enum ServerMode {
    /// reMarkable cloud (default)
    #[default]
    Cloud,
    /// Local server with custom URL
    Local {
        url: String,
        skip_tls_verify: bool,
    },
}

/// Sync client
pub struct SyncClient {
    client: Client,
    device_token: Option<DeviceToken>,
    user_token: Option<UserToken>,
    region: String,
    server_mode: ServerMode,
}

impl SyncClient {
    /// Create a new sync client for cloud
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            device_token: None,
            user_token: None,
            region: "eu".to_string(),
            server_mode: ServerMode::default(),
        }
    }
    
    /// Create a sync client for a local server
    pub fn local(server_url: impl Into<String>) -> Result<Self, SyncError> {
        Self::local_with_options(server_url, false)
    }
    
    /// Create a sync client for a local server with TLS options
    pub fn local_with_options(server_url: impl Into<String>, skip_tls_verify: bool) -> Result<Self, SyncError> {
        let url = server_url.into();
        
        let client = if skip_tls_verify {
            Client::builder()
                .danger_accept_invalid_certs(true)
                .build()?
        } else {
            Client::new()
        };
        
        Ok(Self {
            client,
            device_token: None,
            user_token: None,
            region: "local".to_string(),
            server_mode: ServerMode::Local {
                url,
                skip_tls_verify,
            },
        })
    }
    
    /// Set server mode
    pub fn with_server_mode(mut self, mode: ServerMode) -> Self {
        self.server_mode = mode;
        self
    }
    
    /// Check if using local server
    pub fn is_local(&self) -> bool {
        matches!(self.server_mode, ServerMode::Local { .. })
    }
    
    /// Set device token
    pub fn with_device_token(mut self, token: DeviceToken) -> Self {
        self.device_token = Some(token);
        self
    }
    
    /// Set user token
    pub fn with_user_token(mut self, token: UserToken) -> Self {
        self.region = token.region.clone();
        self.user_token = Some(token);
        self
    }
    
    /// Set region
    pub fn with_region(mut self, region: impl Into<String>) -> Self {
        self.region = region.into();
        self
    }
    
    /// Load tokens from files (convenience method)
    pub fn from_token_files(device_token_path: &str, user_token_path: &str) -> Result<Self, SyncError> {
        let device_token = std::fs::read_to_string(device_token_path)?;
        let user_token = std::fs::read_to_string(user_token_path)?;
        
        // Parse region from user token
        let region = auth::parse_jwt_claim(&user_token, "tectonic")
            .unwrap_or_else(|| "eu".to_string());
        let scopes = auth::parse_jwt_scopes(&user_token);
        
        Ok(Self::new()
            .with_device_token(DeviceToken { token: device_token.trim().to_string() })
            .with_user_token(UserToken {
                token: user_token.trim().to_string(),
                region,
                scopes,
            }))
    }
    
    /// Load tokens from files for a local server
    pub fn from_token_files_local(
        device_token_path: &str,
        user_token_path: &str,
        server_url: &str,
        skip_tls_verify: bool,
    ) -> Result<Self, SyncError> {
        let device_token = std::fs::read_to_string(device_token_path)?;
        let user_token = std::fs::read_to_string(user_token_path)?;
        
        let scopes = auth::parse_jwt_scopes(&user_token);
        
        let mut client = Self::local_with_options(server_url, skip_tls_verify)?;
        client.device_token = Some(DeviceToken { token: device_token.trim().to_string() });
        client.user_token = Some(UserToken {
            token: user_token.trim().to_string(),
            region: "local".to_string(),
            scopes,
        });
        
        Ok(client)
    }
    
    /// Get the base URL for the current region or local server
    fn base_url(&self) -> String {
        match &self.server_mode {
            ServerMode::Cloud => SYNC_API_BASE.replace("{region}", &self.region),
            ServerMode::Local { url, .. } => url.trim_end_matches('/').to_string(),
        }
    }
    
    /// Get authorization header
    fn auth_header(&self) -> Result<String, SyncError> {
        if let Some(token) = &self.user_token {
            Ok(format!("Bearer {}", token.token))
        } else if let Some(token) = &self.device_token {
            Ok(format!("Bearer {}", token.token))
        } else {
            Err(SyncError::AuthRequired)
        }
    }
    
    /// Get sync root (current hash and generation)
    pub async fn get_root(&self) -> Result<SyncRoot, SyncError> {
        let url = format!("{}/sync/v3/root", self.base_url());
        
        let resp = self.client
            .get(&url)
            .header(header::AUTHORIZATION, self.auth_header()?)
            .send()
            .await?;
        
        match resp.status().as_u16() {
            200 => Ok(resp.json().await?),
            401 => Err(SyncError::TokenExpired),
            429 => Err(SyncError::RateLimited),
            status => Err(SyncError::Server {
                status,
                message: resp.text().await.unwrap_or_default(),
            }),
        }
    }
    
    /// Update sync root (push new hash)
    /// 
    /// Uses optimistic locking with expected generation.
    pub async fn update_root(&self, hash: &str, generation: u64) -> Result<(), SyncError> {
        let url = format!("{}/sync/v3/root", self.base_url());
        
        #[derive(Serialize)]
        struct RootUpdate {
            hash: String,
            generation: u64,
        }
        
        let body = RootUpdate {
            hash: hash.to_string(),
            generation,
        };
        
        let resp = self.client
            .put(&url)
            .header(header::AUTHORIZATION, self.auth_header()?)
            .header(header::CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await?;
        
        match resp.status().as_u16() {
            200 | 201 | 204 => Ok(()),
            401 => Err(SyncError::TokenExpired),
            409 => {
                // Generation mismatch - concurrent update
                let current = self.get_root().await?;
                Err(SyncError::Conflict {
                    local_gen: generation,
                    remote_gen: current.generation,
                })
            }
            429 => Err(SyncError::RateLimited),
            status => Err(SyncError::Server {
                status,
                message: resp.text().await.unwrap_or_default(),
            }),
        }
    }
    
    /// Download a file by hash
    /// 
    /// IMPORTANT: The rm-filename header is required by the API.
    /// The value must be JUST the filename, NOT a path:
    ///   ✅ "root.docSchema"
    ///   ✅ "page-uuid.rm"
    ///   ❌ "doc-id/root.docSchema" (path style fails!)
    ///   ❌ "" (empty fails)
    pub async fn download_file(&self, hash: &str, rm_filename: &str) -> Result<Vec<u8>, SyncError> {
        if rm_filename.is_empty() {
            return Err(SyncError::MissingFilename);
        }
        
        // Extract just the filename if a path was passed
        let filename = rm_filename.split('/').last().unwrap_or(rm_filename);
        
        let url = format!("{}/sync/v3/files/{}", self.base_url(), hash);
        
        let resp = self.client
            .get(&url)
            .header(header::AUTHORIZATION, self.auth_header()?)
            .header("rm-filename", filename)
            .send()
            .await?;
        
        match resp.status().as_u16() {
            200 => Ok(resp.bytes().await?.to_vec()),
            401 => Err(SyncError::TokenExpired),
            404 => Err(SyncError::NotFound(hash.to_string())),
            429 => Err(SyncError::RateLimited),
            status => Err(SyncError::Server {
                status,
                message: resp.text().await.unwrap_or_default(),
            }),
        }
    }
    
    /// Upload a file with proper checksum headers
    /// 
    /// Returns the hash of the uploaded file.
    /// 
    /// # Headers sent
    /// - `Authorization`: Bearer token
    /// - `rm-filename`: File identifier  
    /// - `x-goog-hash`: CRC32C checksum for GCS verification
    /// - `rm-parent-hash`: Parent hash for tree linkage (if context provided)
    /// - `rm-sync-id`: Sync session ID (if context provided)
    /// - `rm-batch-number`: Batch operation number (if context provided)
    pub async fn upload_file(
        &self,
        data: &[u8],
        rm_filename: &str,
    ) -> Result<UploadResult, SyncError> {
        self.upload_file_with_context(data, rm_filename, None).await
    }
    
    /// Upload a file with upload context for batch operations
    pub async fn upload_file_with_context(
        &self,
        data: &[u8],
        rm_filename: &str,
        ctx: Option<&mut UploadContext>,
    ) -> Result<UploadResult, SyncError> {
        if rm_filename.is_empty() {
            return Err(SyncError::MissingFilename);
        }
        
        // Calculate SHA-256 hash for content addressing
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(data);
        let hash = hex::encode(hasher.finalize());
        
        // Calculate CRC32C for GCS verification
        let crc_header = x_goog_hash_header(data);
        
        // Extract just filename (no path)
        let filename = rm_filename.split('/').last().unwrap_or(rm_filename);
        
        let url = format!("{}/sync/v3/files/{}", self.base_url(), hash);
        
        let mut req = self.client
            .put(&url)
            .header(header::AUTHORIZATION, self.auth_header()?)
            .header("rm-filename", filename)
            .header("x-goog-hash", crc_header)
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .header(header::CONTENT_LENGTH, data.len());
        
        // Add context headers if provided
        if let Some(context) = ctx {
            req = req
                .header("rm-parent-hash", &context.parent_hash)
                .header("rm-sync-id", &context.sync_id)
                .header("rm-batch-number", context.next_batch().to_string());
            
            if let Some(gen) = context.expect_generation {
                req = req.header("rm-expect-version", gen.to_string());
            }
        }
        
        let resp = req.body(data.to_vec()).send().await?;
        
        match resp.status().as_u16() {
            200 | 201 => Ok(UploadResult {
                hash,
                size: data.len() as u64,
            }),
            401 => Err(SyncError::TokenExpired),
            409 => Err(SyncError::Conflict {
                local_gen: 0,
                remote_gen: 0,
            }),
            429 => Err(SyncError::RateLimited),
            status => Err(SyncError::Server {
                status,
                message: resp.text().await.unwrap_or_default(),
            }),
        }
    }
    
    /// List all documents from the root index
    pub async fn list_documents(&self) -> Result<Vec<DocEntry>, SyncError> {
        // Get root hash
        let root = self.get_root().await?;
        
        // Download the root index
        let root_data = self.download_file(&root.hash, "root.docSchema").await?;
        
        // Parse as JSON array of DocEntry
        let docs: Vec<DocEntry> = serde_json::from_slice(&root_data)
            .map_err(|e| SyncError::Parse(e.to_string()))?;
        
        Ok(docs)
    }
    
    /// Get document schema (hash tree for a single document)
    pub async fn get_document_schema(&self, doc_hash: &str, doc_id: &str) -> Result<DocumentSchema, SyncError> {
        // Schema file is named with doc_id prefix  
        let schema_filename = format!("{}.docSchema", doc_id);
        let schema_data = self.download_file(doc_hash, &schema_filename).await?;
        let schema_str = String::from_utf8_lossy(&schema_data);
        
        DocumentSchema::parse(&schema_str)
    }
    
    /// Download a complete document with all its files
    /// 
    /// This performs full hash tree traversal:
    /// 1. Get document entry from root index
    /// 2. Download document schema (schema.txt)
    /// 3. Download all files listed in schema (.content, .metadata, .rm files)
    pub async fn download_document(&self, doc_id: &str) -> Result<DownloadedDocument, SyncError> {
        // Get document entry from root
        let docs = self.list_documents().await?;
        let doc_entry = docs.iter()
            .find(|d| d.uuid == doc_id)
            .ok_or_else(|| SyncError::NotFound(doc_id.to_string()))?;
        
        // Get document schema
        let schema = self.get_document_schema(&doc_entry.hash, doc_id).await?;
        
        // Download each file in the schema
        let mut metadata: Option<DocMetadata> = None;
        let mut content: Option<ContentFile> = None;
        let mut pages: HashMap<String, Vec<u8>> = HashMap::new();
        let mut pdf: Option<Vec<u8>> = None;
        
        for file in &schema.files {
            let data = self.download_file(&file.hash, &file.filename).await?;
            
            if file.filename.ends_with(".metadata") {
                metadata = serde_json::from_slice(&data).ok();
            } else if file.filename.ends_with(".content") {
                content = serde_json::from_slice(&data).ok();
            } else if file.filename.ends_with(".rm") {
                // Extract page ID from filename: {doc_id}/{page_id}.rm
                if let Some(page_part) = file.filename.split('/').last() {
                    if let Some(page_id) = page_part.strip_suffix(".rm") {
                        pages.insert(page_id.to_string(), data);
                    }
                }
            } else if file.filename.ends_with(".pdf") {
                pdf = Some(data);
            }
        }
        
        Ok(DownloadedDocument {
            id: doc_id.to_string(),
            metadata,
            content,
            pages,
            pdf,
        })
    }
    
    /// Download all documents to a directory
    pub async fn download_all(&self, output_dir: &str) -> Result<Vec<String>, SyncError> {
        use std::fs;
        use std::path::Path;
        
        fs::create_dir_all(output_dir)?;
        
        let docs = self.list_documents().await?;
        let mut downloaded = Vec::new();
        
        for doc_entry in &docs {
            let doc_id = &doc_entry.uuid;
            let doc_dir = Path::new(output_dir).join(doc_id);
            fs::create_dir_all(&doc_dir)?;
            
            // Get document schema
            let schema = match self.get_document_schema(&doc_entry.hash, doc_id).await {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Failed to get schema for {}: {}", doc_id, e);
                    continue;
                }
            };
            
            // Download each file
            for file in &schema.files {
                let data = match self.download_file(&file.hash, &file.filename).await {
                    Ok(d) => d,
                    Err(e) => {
                        eprintln!("Failed to download {}: {}", file.filename, e);
                        continue;
                    }
                };
                
                // Determine output path
                let out_path = if file.filename.contains('/') {
                    // Page file: {doc_id}/{page_id}.rm -> {output_dir}/{doc_id}/{page_id}.rm
                    let page_name = file.filename.split('/').last().unwrap();
                    doc_dir.join(page_name)
                } else {
                    // Top-level file: {doc_id}.content -> {output_dir}/{doc_id}.content
                    Path::new(output_dir).join(&file.filename)
                };
                
                fs::write(&out_path, &data)?;
            }
            
            downloaded.push(doc_id.clone());
        }
        
        Ok(downloaded)
    }
}

impl Default for SyncClient {
    fn default() -> Self {
        Self::new()
    }
}

impl ServerMode {
    /// Create local server mode
    pub fn local(url: impl Into<String>) -> Self {
        Self::Local {
            url: url.into(),
            skip_tls_verify: false,
        }
    }
    
    /// Create local server mode with TLS skip
    pub fn local_insecure(url: impl Into<String>) -> Self {
        Self::Local {
            url: url.into(),
            skip_tls_verify: true,
        }
    }
}

/// Token management
pub mod auth {
    use super::*;
    
    /// Device pairing request
    #[derive(Debug, Serialize)]
    pub struct PairRequest {
        pub code: String,
        #[serde(rename = "deviceDesc")]
        pub device_desc: String,
        #[serde(rename = "deviceID")]
        pub device_id: String,
    }
    
    /// Token response
    #[derive(Debug, Deserialize)]
    pub struct TokenResponse {
        pub token: String,
    }
    
    /// Exchange a one-time code for device token (cloud)
    pub async fn pair_device(
        client: &Client,
        code: &str,
        device_id: &str,
    ) -> Result<DeviceToken, SyncError> {
        pair_device_with_server(client, code, device_id, AUTH_API_BASE).await
    }
    
    /// Exchange a one-time code for device token (configurable server)
    pub async fn pair_device_with_server(
        client: &Client,
        code: &str,
        device_id: &str,
        server_url: &str,
    ) -> Result<DeviceToken, SyncError> {
        let url = format!("{}/token/json/2/device/new", server_url.trim_end_matches('/'));
        
        let req = PairRequest {
            code: code.to_string(),
            device_desc: "remarkable".to_string(),
            device_id: device_id.to_string(),
        };
        
        let resp = client
            .post(&url)
            .json(&req)
            .send()
            .await?;
        
        match resp.status().as_u16() {
            200 => {
                // Try to parse as JSON first
                let text = resp.text().await?;
                if let Ok(token_resp) = serde_json::from_str::<TokenResponse>(&text) {
                    Ok(DeviceToken { token: token_resp.token })
                } else {
                    // Plain text token
                    Ok(DeviceToken { token: text.trim().to_string() })
                }
            }
            status => Err(SyncError::Server {
                status,
                message: resp.text().await.unwrap_or_default(),
            }),
        }
    }
    
    /// Refresh user token using device token (cloud)
    pub async fn refresh_user_token(
        client: &Client,
        device_token: &DeviceToken,
    ) -> Result<UserToken, SyncError> {
        refresh_user_token_with_server(client, device_token, AUTH_API_BASE).await
    }
    
    /// Refresh user token using device token (configurable server)
    pub async fn refresh_user_token_with_server(
        client: &Client,
        device_token: &DeviceToken,
        server_url: &str,
    ) -> Result<UserToken, SyncError> {
        let url = format!("{}/token/json/2/user/new", server_url.trim_end_matches('/'));
        
        let resp = client
            .post(&url)
            .header(header::AUTHORIZATION, format!("Bearer {}", device_token.token))
            .send()
            .await?;
        
        match resp.status().as_u16() {
            200 => {
                let token: String = resp.text().await?;
                // Parse JWT to extract region and scopes (local server may use different format)
                let region = parse_jwt_claim(&token, "tectonic")
                    .unwrap_or_else(|| "local".to_string());
                let scopes = parse_jwt_scopes(&token);
                
                Ok(UserToken { token, region, scopes })
            }
            401 => Err(SyncError::TokenExpired),
            status => Err(SyncError::Server {
                status,
                message: resp.text().await.unwrap_or_default(),
            }),
        }
    }
    
    /// Parse a claim from a JWT (basic implementation)
    pub fn parse_jwt_claim(token: &str, claim: &str) -> Option<String> {
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 3 {
            return None;
        }
        
        use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
        let payload = URL_SAFE_NO_PAD.decode(parts[1]).ok()?;
        let json: serde_json::Value = serde_json::from_slice(&payload).ok()?;
        
        // Check for nested claim (e.g., https://auth.remarkable.com/tectonic)
        if let Some(val) = json.get(claim).and_then(|v| v.as_str()) {
            return Some(val.to_string());
        }
        
        // Check for remarkable-specific claim format
        let rm_claim = format!("https://auth.remarkable.com/{}", claim);
        json.get(&rm_claim)?.as_str().map(|s| s.to_string())
    }
    
    /// Parse scopes from JWT
    pub fn parse_jwt_scopes(token: &str) -> Vec<String> {
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 3 {
            return vec![];
        }
        
        use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
        let Ok(payload) = URL_SAFE_NO_PAD.decode(parts[1]) else { return vec![] };
        let Ok(json): Result<serde_json::Value, _> = serde_json::from_slice(&payload) else { return vec![] };
        
        json.get("scopes")
            .and_then(|v| v.as_str())
            .map(|s| s.split_whitespace().map(|s| s.to_string()).collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_document_schema_parse() {
        let schema_txt = r#"3
05e0a57ed297f7155c178c72566f7a7570874634946c4d65942c105e7154ce11:0:doc-id.content:0:3952
2942ade720c85698f86b3626ed6294bb689afe8c9f96fa108929d04cbb0f4fed:0:doc-id.metadata:0:236
308e22ecc1f8782c331d7dc17f1f7f2caf6ec7268758794a984cad8fdf020622:0:doc-id/page-id.rm:0:319676"#;
        
        let schema = DocumentSchema::parse(schema_txt).unwrap();
        assert_eq!(schema.files.len(), 3);
        
        assert_eq!(schema.files[0].filename, "doc-id.content");
        assert_eq!(schema.files[0].size, 3952);
        
        assert_eq!(schema.files[1].filename, "doc-id.metadata");
        assert_eq!(schema.files[1].size, 236);
        
        assert_eq!(schema.files[2].filename, "doc-id/page-id.rm");
        assert_eq!(schema.files[2].size, 319676);
    }
    
    #[test]
    fn test_document_schema_empty() {
        let result = DocumentSchema::parse("");
        assert!(result.is_err());
    }
    
    #[test]
    fn test_upload_context_batch() {
        let mut ctx = UploadContext::new("abc123".to_string());
        assert_eq!(ctx.next_batch(), 0);
        assert_eq!(ctx.next_batch(), 1);
        assert_eq!(ctx.next_batch(), 2);
    }
}
