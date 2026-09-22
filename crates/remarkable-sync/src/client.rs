//! Sync client for reMarkable cloud
//!
//! Implements the sync protocol for downloading and uploading documents.
//!
//! The sync protocol uses:
//! - Merkle tree hash structure for efficient delta sync
//! - rm-filename header for file identification  
//! - SHA-256 content hashes for deduplication
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
use serde::{Deserialize, Serialize};
use thiserror::Error;
use std::collections::HashMap;

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

/// Sync client
pub struct SyncClient {
    client: Client,
    device_token: Option<DeviceToken>,
    user_token: Option<UserToken>,
    region: String,
}

impl SyncClient {
    /// Create a new sync client
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            device_token: None,
            user_token: None,
            region: "eu".to_string(),
        }
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
    
    /// Get the base URL for the current region
    fn base_url(&self) -> String {
        SYNC_API_BASE.replace("{region}", &self.region)
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
        
        let url = format!("{}/sync/v3/files/{}", self.base_url(), hash);
        
        let resp = self.client
            .get(&url)
            .header(header::AUTHORIZATION, self.auth_header()?)
            .header("rm-filename", rm_filename)
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
        // The document hash points to a schema.txt file
        let schema_filename = format!("{}/schema.txt", doc_id);
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
    
    /// Upload a file
    /// 
    /// Returns the hash of the uploaded file
    pub async fn upload_file(&self, data: &[u8], rm_filename: &str) -> Result<String, SyncError> {
        if rm_filename.is_empty() {
            return Err(SyncError::MissingFilename);
        }
        
        // Calculate SHA-256 hash
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(data);
        let hash = format!("{:x}", hasher.finalize());
        
        let url = format!("{}/sync/v3/files/{}", self.base_url(), hash);
        
        let resp = self.client
            .put(&url)
            .header(header::AUTHORIZATION, self.auth_header()?)
            .header("rm-filename", rm_filename)
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .body(data.to_vec())
            .send()
            .await?;
        
        match resp.status().as_u16() {
            200 | 201 => Ok(hash),
            401 => Err(SyncError::TokenExpired),
            429 => Err(SyncError::RateLimited),
            status => Err(SyncError::Server {
                status,
                message: resp.text().await.unwrap_or_default(),
            }),
        }
    }
}

impl Default for SyncClient {
    fn default() -> Self {
        Self::new()
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
    
    /// Exchange a one-time code for device token
    pub async fn pair_device(
        client: &Client,
        code: &str,
        device_id: &str,
    ) -> Result<DeviceToken, SyncError> {
        let url = "https://webapp.cloud.remarkable.engineering/token/json/2/device/new";
        
        let req = PairRequest {
            code: code.to_string(),
            device_desc: "remarkable".to_string(),
            device_id: device_id.to_string(),
        };
        
        let resp = client
            .post(url)
            .json(&req)
            .send()
            .await?;
        
        match resp.status().as_u16() {
            200 => {
                let token_resp: TokenResponse = resp.json().await?;
                Ok(DeviceToken { token: token_resp.token })
            }
            status => Err(SyncError::Server {
                status,
                message: resp.text().await.unwrap_or_default(),
            }),
        }
    }
    
    /// Refresh user token using device token
    pub async fn refresh_user_token(
        client: &Client,
        device_token: &DeviceToken,
    ) -> Result<UserToken, SyncError> {
        let url = "https://webapp.cloud.remarkable.engineering/token/json/2/user/new";
        
        let resp = client
            .post(url)
            .header(header::AUTHORIZATION, format!("Bearer {}", device_token.token))
            .send()
            .await?;
        
        match resp.status().as_u16() {
            200 => {
                let token: String = resp.text().await?;
                // Parse JWT to extract region and scopes
                let region = parse_jwt_claim(&token, "tectonic")
                    .unwrap_or_else(|| "eu".to_string());
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
        
        json.get(claim)?.as_str().map(|s| s.to_string())
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
}


use remarkable_core::{DocumentMetadata, DocumentContent};

impl SyncClient {
    /// Parse document metadata from .metadata file contents
    pub fn parse_metadata(content: &str) -> Result<DocumentMetadata, SyncError> {
        serde_json::from_str(content)
            .map_err(|e| SyncError::Parse(format!("Failed to parse metadata: {}", e)))
    }
    
    /// Parse document content from .content file contents
    pub fn parse_content(content: &str) -> Result<DocumentContent, SyncError> {
        serde_json::from_str(content)
            .map_err(|e| SyncError::Parse(format!("Failed to parse content: {}", e)))
    }
    
    /// Get full document info including metadata and content
    pub async fn get_document_info(&self, doc_id: &str) -> Result<(DocumentMetadata, DocumentContent), SyncError> {
        // Get the document hash from root
        let docs = self.list_documents().await?;
        let doc = docs.iter()
            .find(|d| d.uuid == doc_id)
            .ok_or_else(|| SyncError::Parse(format!("Document {} not found", doc_id)))?;
        
        // Download metadata and content files
        let metadata_content = self.download_file(&doc.hash, &format!("{}.metadata", doc_id)).await?;
        let content_content = self.download_file(&doc.hash, &format!("{}.content", doc_id)).await?;
        
        let metadata = Self::parse_metadata(&String::from_utf8_lossy(&metadata_content))?;
        let content = Self::parse_content(&String::from_utf8_lossy(&content_content))?;
        
        Ok((metadata, content))
    }
}

#[cfg(test)]
mod metadata_tests {
    use super::*;
    use remarkable_core::{DocumentMetadata, DocumentContent};
    
    #[test]
    fn test_parse_metadata() {
        let json = r#"{
            "createdTime": "1715695544486",
            "lastModified": "1718759769109",
            "lastOpened": "1729709212944",
            "lastOpenedPage": 4,
            "parent": "",
            "pinned": false,
            "type": "DocumentType",
            "visibleName": "brainstorm"
        }"#;
        
        let metadata: DocumentMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(metadata.visible_name, "brainstorm");
        assert!(metadata.is_document());
        assert!(!metadata.is_folder());
    }
    
    #[test]
    fn test_parse_folder_metadata() {
        let json = r#"{
            "createdTime": "1706121472417",
            "lastModified": "1706121472406",
            "parent": "",
            "pinned": false,
            "type": "CollectionType",
            "visibleName": "Folder 3"
        }"#;
        
        let metadata: DocumentMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(metadata.visible_name, "Folder 3");
        assert!(metadata.is_folder());
        assert!(!metadata.is_document());
    }
    
    #[test]
    fn test_parse_content() {
        let json = r#"{
            "cPages": {
                "pages": [
                    {"id": "page1", "idx": {"timestamp": "2:2", "value": "ba"}},
                    {"id": "page2", "idx": {"timestamp": "2:2", "value": "bb"}}
                ],
                "uuids": []
            },
            "pageCount": 2,
            "fileType": "notebook",
            "tags": []
        }"#;
        
        let content: DocumentContent = serde_json::from_str(json).unwrap();
        assert_eq!(content.page_count, Some(2));
        let pages = content.page_ids();
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0], "page1");
        assert_eq!(pages[1], "page2");
    }
}

