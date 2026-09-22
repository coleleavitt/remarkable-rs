//! Sync protocol trait definitions

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use crate::error::SyncError;
use crate::protocol::SyncVersion;

/// Root information from sync endpoint
#[derive(Debug, Clone)]
pub struct RootInfo {
    /// Root hash or document list identifier
    pub hash: String,
    /// Generation counter (for optimistic locking)
    pub generation: u64,
    /// Schema version
    pub schema_version: u32,
    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

impl RootInfo {
    /// Create a new RootInfo
    pub fn new(hash: String) -> Self {
        Self {
            hash,
            generation: 0,
            schema_version: 3,
            metadata: HashMap::new(),
        }
    }
    
    /// Create with generation
    pub fn with_generation(mut self, gen: u64) -> Self {
        self.generation = gen;
        self
    }
    
    /// Create with schema version
    pub fn with_schema_version(mut self, version: u32) -> Self {
        self.schema_version = version;
        self
    }
}

/// Document entry in a document list
#[derive(Debug, Clone)]
pub struct DocumentInfo {
    /// Document UUID
    pub id: String,
    /// Document type (DocumentType, CollectionType)
    pub doc_type: String,
    /// Document name
    pub name: Option<String>,
    /// Parent folder ID
    pub parent: Option<String>,
    /// Content hash (for hash-based protocols)
    pub hash: Option<String>,
    /// Version number
    pub version: u32,
    /// Last modified timestamp
    pub modified_at: Option<String>,
    /// Whether document is deleted (soft delete)
    pub deleted: bool,
    /// Additional metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

impl DocumentInfo {
    /// Create a new document info
    pub fn new(id: String) -> Self {
        Self {
            id,
            doc_type: "DocumentType".to_string(),
            name: None,
            parent: None,
            hash: None,
            version: 1,
            modified_at: None,
            deleted: false,
            metadata: HashMap::new(),
        }
    }
    
    /// Check if this is a folder/collection
    pub fn is_folder(&self) -> bool {
        self.doc_type == "CollectionType"
    }
    
    /// Check if this is a document
    pub fn is_document(&self) -> bool {
        self.doc_type == "DocumentType"
    }
}

/// Downloaded document content
#[derive(Debug, Clone)]
pub struct Document {
    /// Document UUID
    pub id: String,
    /// Document info
    pub info: DocumentInfo,
    /// Content files by name
    pub files: HashMap<String, Vec<u8>>,
    /// Page data by page ID
    pub pages: HashMap<String, Vec<u8>>,
}

impl Document {
    /// Create a new document
    pub fn new(id: String) -> Self {
        Self {
            id: id.clone(),
            info: DocumentInfo::new(id),
            files: HashMap::new(),
            pages: HashMap::new(),
        }
    }
    
    /// Get metadata file content
    pub fn metadata(&self) -> Option<&Vec<u8>> {
        self.files.get(".metadata")
            .or_else(|| self.files.get("metadata"))
    }
    
    /// Get content file 
    pub fn content(&self) -> Option<&Vec<u8>> {
        self.files.get(".content")
            .or_else(|| self.files.get("content"))
    }
    
    /// Get PDF if present
    pub fn pdf(&self) -> Option<&Vec<u8>> {
        self.files.iter()
            .find(|(k, _)| k.ends_with(".pdf"))
            .map(|(_, v)| v)
    }
}

/// Result of a sync operation
#[derive(Debug, Clone)]
pub struct SyncResult {
    /// Documents that were downloaded
    pub downloaded: Vec<String>,
    /// Documents that were uploaded
    pub uploaded: Vec<String>,
    /// Documents that were deleted
    pub deleted: Vec<String>,
    /// New generation after sync
    pub generation: u64,
    /// Whether there were conflicts
    pub had_conflicts: bool,
    /// Conflict details
    pub conflicts: Vec<SyncConflict>,
}

impl SyncResult {
    /// Create an empty sync result
    pub fn empty() -> Self {
        Self {
            downloaded: Vec::new(),
            uploaded: Vec::new(),
            deleted: Vec::new(),
            generation: 0,
            had_conflicts: false,
            conflicts: Vec::new(),
        }
    }
}

/// Details about a sync conflict
#[derive(Debug, Clone)]
pub struct SyncConflict {
    /// Document ID that had conflict
    pub document_id: String,
    /// Local version
    pub local_version: u32,
    /// Server version
    pub server_version: u32,
    /// How it was resolved
    pub resolution: ConflictResolution,
}

/// How a conflict was resolved
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictResolution {
    /// Used local version
    UseLocal,
    /// Used server version
    UseServer,
    /// Created a copy
    Duplicate,
    /// Merged changes
    Merge,
    /// Unresolved
    Pending,
}

/// Result of an upload operation
#[derive(Debug, Clone)]
pub struct UploadResult {
    /// Document ID
    pub document_id: String,
    /// Content hash (for hash-based protocols)
    pub hash: Option<String>,
    /// New version number
    pub version: u32,
    /// Upload size in bytes
    pub size: u64,
}

/// Type alias for async boxed future
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Core sync protocol trait
///
/// Implementations provide version-specific sync behavior while
/// presenting a unified interface.
pub trait SyncProtocol: Send + Sync {
    /// Get the protocol version
    fn version(&self) -> SyncVersion;
    
    /// Get the base URL being used
    fn base_url(&self) -> &str;
    
    /// Check if connected/authenticated
    fn is_authenticated(&self) -> bool;
    
    /// Get root information (hash, generation, etc.)
    fn get_root(&self) -> BoxFuture<'_, Result<RootInfo, SyncError>>;
    
    /// List all documents
    fn list_documents(&self) -> BoxFuture<'_, Result<Vec<DocumentInfo>, SyncError>>;
    
    /// Download a single document with all its files
    fn download_document(&self, id: &str) -> BoxFuture<'_, Result<Document, SyncError>>;
    
    /// Upload a document
    fn upload_document(&self, doc: &Document) -> BoxFuture<'_, Result<UploadResult, SyncError>>;
    
    /// Delete a document
    fn delete_document(&self, id: &str) -> BoxFuture<'_, Result<(), SyncError>>;
    
    /// Perform full sync
    fn sync(&self) -> BoxFuture<'_, Result<SyncResult, SyncError>>;
    
    // Optional operations with default implementations
    
    /// Download a file by hash (for hash-based protocols)
    fn download_file(&self, _hash: &str, _filename: &str) -> BoxFuture<'_, Result<Vec<u8>, SyncError>> {
        Box::pin(async {
            Err(SyncError::NotFound("download_file not supported by this protocol version".into()))
        })
    }
    
    /// Upload a file (for hash-based protocols)
    fn upload_file(&self, _data: &[u8], _filename: &str) -> BoxFuture<'_, Result<String, SyncError>> {
        Box::pin(async {
            Err(SyncError::NotFound("upload_file not supported by this protocol version".into()))
        })
    }
    
    /// Update the root hash (for hash-based protocols)
    fn update_root(&self, _hash: &str, _generation: u64) -> BoxFuture<'_, Result<(), SyncError>> {
        Box::pin(async {
            Err(SyncError::NotFound("update_root not supported by this protocol version".into()))
        })
    }
    
    /// Create a folder
    fn create_folder(&self, name: &str, parent: Option<&str>) -> BoxFuture<'_, Result<DocumentInfo, SyncError>> {
        let name = name.to_string();
        let parent = parent.map(|s| s.to_string());
        Box::pin(async move {
            let _ = (name, parent);
            Err(SyncError::NotFound("create_folder not implemented".into()))
        })
    }
    
    /// Rename a document
    fn rename_document(&self, id: &str, new_name: &str) -> BoxFuture<'_, Result<(), SyncError>> {
        let id = id.to_string();
        let new_name = new_name.to_string();
        Box::pin(async move {
            let _ = (id, new_name);
            Err(SyncError::NotFound("rename_document not implemented".into()))
        })
    }
    
    /// Move a document to a different folder
    fn move_document(&self, id: &str, new_parent: Option<&str>) -> BoxFuture<'_, Result<(), SyncError>> {
        let id = id.to_string();
        let new_parent = new_parent.map(|s| s.to_string());
        Box::pin(async move {
            let _ = (id, new_parent);
            Err(SyncError::NotFound("move_document not implemented".into()))
        })
    }
}

/// Configuration for sync protocol
#[derive(Debug, Clone)]
pub struct SyncConfig {
    /// Base URL for API
    pub base_url: String,
    /// Device token
    pub device_token: Option<String>,
    /// User token
    pub user_token: Option<String>,
    /// Tectonic region (for V3+)
    pub region: Option<String>,
    /// Skip TLS verification (for local server)
    pub skip_tls_verify: bool,
    /// Preferred protocol version
    pub preferred_version: Option<SyncVersion>,
    /// Request timeout in seconds
    pub timeout_secs: u64,
}

impl SyncConfig {
    /// Create config for reMarkable cloud
    pub fn cloud() -> Self {
        Self {
            base_url: "https://internal.cloud.remarkable.com".to_string(),
            device_token: None,
            user_token: None,
            region: Some("eu".to_string()),
            skip_tls_verify: false,
            preferred_version: None,
            timeout_secs: 30,
        }
    }
    
    /// Create config for local server
    pub fn local(url: impl Into<String>) -> Self {
        Self {
            base_url: url.into(),
            device_token: None,
            user_token: None,
            region: None,
            skip_tls_verify: true,
            preferred_version: None,
            timeout_secs: 30,
        }
    }
    
    /// Set device token
    pub fn with_device_token(mut self, token: impl Into<String>) -> Self {
        self.device_token = Some(token.into());
        self
    }
    
    /// Set user token
    pub fn with_user_token(mut self, token: impl Into<String>) -> Self {
        self.user_token = Some(token.into());
        self
    }
    
    /// Set region
    pub fn with_region(mut self, region: impl Into<String>) -> Self {
        self.region = Some(region.into());
        self
    }
    
    /// Set preferred protocol version
    pub fn with_version(mut self, version: SyncVersion) -> Self {
        self.preferred_version = Some(version);
        self
    }
    
    /// Load tokens from files
    pub fn from_token_files(device_path: &str, user_path: &str) -> Result<Self, SyncError> {
        let device_token = std::fs::read_to_string(device_path)?.trim().to_string();
        let user_token = std::fs::read_to_string(user_path)?.trim().to_string();
        
        // Parse region from user token
        let region = parse_jwt_claim(&user_token, "tectonic")
            .or_else(|| parse_jwt_claim(&user_token, "https://auth.remarkable.com/tectonic"));
        
        Ok(Self::cloud()
            .with_device_token(device_token)
            .with_user_token(user_token)
            .with_region(region.unwrap_or_else(|| "eu".to_string())))
    }
    
    /// Get the sync URL for the configured region
    pub fn sync_url(&self) -> String {
        if let Some(region) = &self.region {
            format!("https://{}.tectonic.remarkable.com", region)
        } else {
            self.base_url.clone()
        }
    }
}

/// Parse a claim from a JWT token (without verification)
pub fn parse_jwt_claim(token: &str, claim: &str) -> Option<String> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    
    let payload = base64::Engine::decode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        parts[1],
    ).ok()?;
    
    let claims: serde_json::Value = serde_json::from_slice(&payload).ok()?;
    claims.get(claim)?.as_str().map(|s| s.to_string())
}
