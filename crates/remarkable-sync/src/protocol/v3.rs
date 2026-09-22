//! Sync V3: Current hash-tree based protocol (tectonic)
//!
//! This is the current production sync protocol using a Merkle-like
//! hash tree structure for efficient delta sync.
//!
//! ## Endpoints
//! - `GET /sync/v3/root` - Get root hash and generation
//! - `GET /sync/v3/files/{hash}` - Download file by content hash
//! - `PUT /sync/v3/files/{hash}` - Upload file by content hash
//!
//! ## Headers
//! - `rm-filename` - Required for all file operations
//! - `x-goog-hash` - CRC32C checksum for uploads
//! - `rm-parent-hash` - Parent hash for tree linkage
//! - `rm-sync-id` - Sync session identifier
//! - `rm-batch-number` - Batch operation counter

use std::collections::HashMap;
use reqwest::{Client, header};
use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};
use uuid::Uuid;

use crate::error::SyncError;
use crate::checksum::x_goog_hash_header;
use crate::protocol::{
    SyncVersion, SyncProtocol, SyncConfig, RootInfo, DocumentInfo,
    Document, SyncResult, UploadResult, BoxFuture,
};

/// V3 root response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3Root {
    pub hash: String,
    pub generation: u64,
    #[serde(rename = "schemaVersion", default)]
    pub schema_version: u32,
}

impl From<V3Root> for RootInfo {
    fn from(root: V3Root) -> Self {
        Self {
            hash: root.hash,
            generation: root.generation,
            schema_version: root.schema_version,
            metadata: HashMap::new(),
        }
    }
}

/// Document entry from root index
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3DocEntry {
    pub hash: String,
    #[serde(rename = "type")]
    pub entry_type: String,
    pub uuid: String,
    pub version: String,
    pub size: String,
}

impl From<V3DocEntry> for DocumentInfo {
    fn from(entry: V3DocEntry) -> Self {
        Self {
            id: entry.uuid,
            doc_type: entry.entry_type,
            name: None,  // Name comes from metadata file
            parent: None,  // Parent comes from metadata file
            hash: Some(entry.hash),
            version: entry.version.parse().unwrap_or(1),
            modified_at: None,
            deleted: false,
            metadata: HashMap::new(),
        }
    }
}

/// File entry from document schema
#[derive(Debug, Clone)]
pub struct V3FileEntry {
    pub hash: String,
    pub filename: String,
    pub size: u64,
}

/// Document schema (schema.txt format)
#[derive(Debug, Clone)]
pub struct V3Schema {
    pub files: Vec<V3FileEntry>,
}

impl V3Schema {
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
        
        let _count: usize = lines[0].trim().parse()
            .map_err(|_| SyncError::Parse("Invalid schema count".into()))?;
        
        let mut files = Vec::new();
        for line in lines.iter().skip(1) {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 5 {
                files.push(V3FileEntry {
                    hash: parts[0].to_string(),
                    filename: parts[2].to_string(),
                    size: parts[4].parse().unwrap_or(0),
                });
            }
        }
        
        Ok(Self { files })
    }
    
    /// Serialize to schema.txt format
    pub fn serialize(&self) -> String {
        let mut output = format!("{}\n", self.files.len());
        for file in &self.files {
            output.push_str(&format!(
                "{}:0:{}:0:{}\n",
                file.hash, file.filename, file.size
            ));
        }
        output
    }
}

/// Document metadata
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct V3Metadata {
    #[serde(rename = "createdTime", default)]
    pub created_time: Option<String>,
    #[serde(rename = "lastModified", default)]
    pub last_modified: Option<String>,
    #[serde(rename = "lastOpened", default)]
    pub last_opened: Option<String>,
    #[serde(rename = "lastOpenedPage", default)]
    pub last_opened_page: Option<u32>,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub pinned: Option<bool>,
    #[serde(rename = "type", default)]
    pub doc_type: Option<String>,
    #[serde(rename = "visibleName", default)]
    pub visible_name: Option<String>,
    #[serde(default)]
    pub deleted: Option<bool>,
}

/// Upload context for batch operations
#[derive(Debug, Clone)]
pub struct V3UploadContext {
    pub sync_id: String,
    pub batch_number: u32,
    pub parent_hash: String,
    pub expect_generation: Option<u64>,
}

impl V3UploadContext {
    pub fn new(parent_hash: String) -> Self {
        Self {
            sync_id: Uuid::new_v4().to_string(),
            batch_number: 0,
            parent_hash,
            expect_generation: None,
        }
    }
    
    pub fn next_batch(&mut self) -> u32 {
        let current = self.batch_number;
        self.batch_number += 1;
        current
    }
}

/// V3 Sync Protocol Client
pub struct SyncV3Client {
    client: Client,
    config: SyncConfig,
}

impl SyncV3Client {
    /// Create a new V3 client
    pub fn new(config: SyncConfig) -> Result<Self, SyncError> {
        let client = if config.skip_tls_verify {
            Client::builder()
                .danger_accept_invalid_certs(true)
                .timeout(std::time::Duration::from_secs(config.timeout_secs))
                .build()?
        } else {
            Client::builder()
                .timeout(std::time::Duration::from_secs(config.timeout_secs))
                .build()?
        };
        
        Ok(Self { client, config })
    }
    
    /// Get authorization header
    fn auth_header(&self) -> Result<String, SyncError> {
        self.config.user_token.as_ref()
            .or(self.config.device_token.as_ref())
            .map(|t| format!("Bearer {}", t))
            .ok_or(SyncError::AuthRequired)
    }
    
    /// Get the sync API base URL
    fn sync_url(&self) -> String {
        self.config.sync_url()
    }
    
    /// Get V3 root
    pub async fn get_v3_root(&self) -> Result<V3Root, SyncError> {
        let url = format!("{}/sync/v3/root", self.sync_url());
        
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
    
    /// Update root hash
    pub async fn update_v3_root(&self, hash: &str, generation: u64) -> Result<(), SyncError> {
        let url = format!("{}/sync/v3/root", self.sync_url());
        
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
                let current = self.get_v3_root().await?;
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
    
    /// Download file by hash
    /// 
    /// The rm-filename header must be JUST the filename, not a path.
    pub async fn download_v3_file(&self, hash: &str, rm_filename: &str) -> Result<Vec<u8>, SyncError> {
        if rm_filename.is_empty() {
            return Err(SyncError::MissingFilename);
        }
        
        // Extract just the filename if a path was passed
        let filename = rm_filename.split('/').last().unwrap_or(rm_filename);
        
        let url = format!("{}/sync/v3/files/{}", self.sync_url(), hash);
        
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
    
    /// Upload file with checksum headers
    pub async fn upload_v3_file(
        &self,
        data: &[u8],
        rm_filename: &str,
        ctx: Option<&mut V3UploadContext>,
    ) -> Result<String, SyncError> {
        if rm_filename.is_empty() {
            return Err(SyncError::MissingFilename);
        }
        
        // Calculate SHA-256 hash
        let mut hasher = Sha256::new();
        hasher.update(data);
        let hash = hex::encode(hasher.finalize());
        
        // Calculate CRC32C for GCS
        let crc_header = x_goog_hash_header(data);
        
        let filename = rm_filename.split('/').last().unwrap_or(rm_filename);
        let url = format!("{}/sync/v3/files/{}", self.sync_url(), hash);
        
        let mut req = self.client
            .put(&url)
            .header(header::AUTHORIZATION, self.auth_header()?)
            .header("rm-filename", filename)
            .header("x-goog-hash", crc_header)
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .header(header::CONTENT_LENGTH, data.len());
        
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
            200 | 201 => Ok(hash),
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
    
    /// List documents from root index
    pub async fn list_v3_docs(&self) -> Result<Vec<V3DocEntry>, SyncError> {
        let root = self.get_v3_root().await?;
        let root_data = self.download_v3_file(&root.hash, "root.docSchema").await?;
        let docs: Vec<V3DocEntry> = serde_json::from_slice(&root_data)
            .map_err(|e| SyncError::Parse(e.to_string()))?;
        Ok(docs)
    }
    
    /// Get document schema
    pub async fn get_doc_schema(&self, doc_hash: &str, doc_id: &str) -> Result<V3Schema, SyncError> {
        let schema_filename = format!("{}.docSchema", doc_id);
        let schema_data = self.download_v3_file(doc_hash, &schema_filename).await?;
        let schema_str = String::from_utf8_lossy(&schema_data);
        V3Schema::parse(&schema_str)
    }
}

impl SyncProtocol for SyncV3Client {
    fn version(&self) -> SyncVersion {
        SyncVersion::V3
    }
    
    fn base_url(&self) -> &str {
        &self.config.base_url
    }
    
    fn is_authenticated(&self) -> bool {
        self.config.user_token.is_some() || self.config.device_token.is_some()
    }
    
    fn get_root(&self) -> BoxFuture<'_, Result<RootInfo, SyncError>> {
        Box::pin(async move {
            let root = self.get_v3_root().await?;
            Ok(root.into())
        })
    }
    
    fn list_documents(&self) -> BoxFuture<'_, Result<Vec<DocumentInfo>, SyncError>> {
        Box::pin(async move {
            let docs = self.list_v3_docs().await?;
            Ok(docs.into_iter().map(DocumentInfo::from).collect())
        })
    }
    
    fn download_document(&self, id: &str) -> BoxFuture<'_, Result<Document, SyncError>> {
        let id = id.to_string();
        Box::pin(async move {
            // Get document entry from root
            let docs = self.list_v3_docs().await?;
            let doc_entry = docs.iter()
                .find(|d| d.uuid == id)
                .ok_or_else(|| SyncError::NotFound(id.clone()))?;
            
            // Get document schema
            let schema = self.get_doc_schema(&doc_entry.hash, &id).await?;
            
            // Download files
            let mut doc = Document::new(id.clone());
            let mut pages = HashMap::new();
            
            for file in &schema.files {
                let data = self.download_v3_file(&file.hash, &file.filename).await?;
                
                if file.filename.ends_with(".rm") {
                    // Extract page ID
                    if let Some(page_part) = file.filename.split('/').last() {
                        if let Some(page_id) = page_part.strip_suffix(".rm") {
                            pages.insert(page_id.to_string(), data);
                        }
                    }
                } else {
                    doc.files.insert(file.filename.clone(), data);
                }
            }
            
            doc.pages = pages;
            
            // Parse metadata if available
            if let Some(meta_data) = doc.files.get(".metadata").or_else(|| doc.files.get("metadata")) {
                if let Ok(metadata) = serde_json::from_slice::<V3Metadata>(meta_data) {
                    doc.info.name = metadata.visible_name;
                    doc.info.parent = metadata.parent;
                    doc.info.doc_type = metadata.doc_type.unwrap_or_else(|| "DocumentType".to_string());
                    doc.info.deleted = metadata.deleted.unwrap_or(false);
                }
            }
            
            Ok(doc)
        })
    }
    
    fn upload_document(&self, doc: &Document) -> BoxFuture<'_, Result<UploadResult, SyncError>> {
        let doc = doc.clone();
        Box::pin(async move {
            // Get current root for parent hash
            let root = self.get_v3_root().await?;
            let mut ctx = V3UploadContext::new(root.hash.clone());
            ctx.expect_generation = Some(root.generation);
            
            let mut total_size: u64 = 0;
            let mut file_hash = String::new();
            
            // Upload all files
            for (filename, data) in &doc.files {
                file_hash = self.upload_v3_file(data, filename, Some(&mut ctx)).await?;
                total_size += data.len() as u64;
            }
            
            // Upload pages
            for (page_id, data) in &doc.pages {
                let filename = format!("{}.rm", page_id);
                self.upload_v3_file(data, &filename, Some(&mut ctx)).await?;
                total_size += data.len() as u64;
            }
            
            Ok(UploadResult {
                document_id: doc.id.clone(),
                hash: Some(file_hash),
                version: doc.info.version + 1,
                size: total_size,
            })
        })
    }
    
    fn delete_document(&self, id: &str) -> BoxFuture<'_, Result<(), SyncError>> {
        let _id = id.to_string();
        Box::pin(async move {
            // V3 delete is done by updating metadata with deleted=true
            // then updating the root hash tree
            Err(SyncError::NotFound("V3 delete requires full tree update".into()))
        })
    }
    
    fn sync(&self) -> BoxFuture<'_, Result<SyncResult, SyncError>> {
        Box::pin(async move {
            let root = self.get_v3_root().await?;
            let _docs = self.list_v3_docs().await?;
            
            Ok(SyncResult {
                downloaded: Vec::new(),
                uploaded: Vec::new(),
                deleted: Vec::new(),
                generation: root.generation,
                had_conflicts: false,
                conflicts: Vec::new(),
            })
        })
    }
    
    fn download_file(&self, hash: &str, filename: &str) -> BoxFuture<'_, Result<Vec<u8>, SyncError>> {
        let hash = hash.to_string();
        let filename = filename.to_string();
        Box::pin(async move {
            self.download_v3_file(&hash, &filename).await
        })
    }
    
    fn upload_file(&self, data: &[u8], filename: &str) -> BoxFuture<'_, Result<String, SyncError>> {
        let data = data.to_vec();
        let filename = filename.to_string();
        Box::pin(async move {
            self.upload_v3_file(&data, &filename, None).await
        })
    }
    
    fn update_root(&self, hash: &str, generation: u64) -> BoxFuture<'_, Result<(), SyncError>> {
        let hash = hash.to_string();
        Box::pin(async move {
            self.update_v3_root(&hash, generation).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_schema_parse() {
        let schema_text = "2\nabc123:0:test.rm:0:100\ndef456:0:other.rm:0:200";
        let schema = V3Schema::parse(schema_text).unwrap();
        assert_eq!(schema.files.len(), 2);
        assert_eq!(schema.files[0].hash, "abc123");
        assert_eq!(schema.files[0].filename, "test.rm");
    }
    
    #[test]
    fn test_v3_root_conversion() {
        let root = V3Root {
            hash: "abc123".to_string(),
            generation: 42,
            schema_version: 3,
        };
        let info: RootInfo = root.into();
        assert_eq!(info.hash, "abc123");
        assert_eq!(info.generation, 42);
    }
}
