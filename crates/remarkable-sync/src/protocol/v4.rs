//! Sync V4: Merkle tree with generation counters (beta firmware)
//!
//! This is the newest sync protocol from beta firmware 3.28+.
//! It extends V3 with delta sync support and enhanced optimistic locking.
//!
//! ## Features
//! - Merkle tree structure with per-node generations
//! - Delta sync (only changed subtrees)
//! - Enhanced optimistic locking
//! - Bidirectional sync support
//!
//! ## Endpoints
//! - `GET /sync/v4/root` - Get root with generation
//! - `GET /sync/v4/files/{hash}` - Download file
//! - `PUT /sync/v4/files/{hash}` - Upload file
//! - `GET /sync/v4/delta/{from_gen}` - Get changes since generation
//! - `POST /sync/v4/commit` - Atomic commit with generation check

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

/// V4 root response with enhanced metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V4Root {
    pub hash: String,
    pub generation: u64,
    #[serde(rename = "schemaVersion", default)]
    pub schema_version: u32,
    /// Per-node generation tracking
    #[serde(default)]
    pub node_generations: HashMap<String, u64>,
    /// Delta sync supported
    #[serde(default)]
    pub delta_supported: bool,
}

impl From<V4Root> for RootInfo {
    fn from(root: V4Root) -> Self {
        let mut metadata = HashMap::new();
        metadata.insert("delta_supported".to_string(), root.delta_supported.to_string());
        
        Self {
            hash: root.hash,
            generation: root.generation,
            schema_version: root.schema_version,
            metadata,
        }
    }
}

/// Delta entry representing a change
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaEntry {
    pub id: String,
    pub hash: Option<String>,
    pub generation: u64,
    pub op: DeltaOp,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

/// Delta operation type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeltaOp {
    Create,
    Update,
    Delete,
    Move,
}

/// Delta response from server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaResponse {
    pub from_generation: u64,
    pub to_generation: u64,
    pub entries: Vec<DeltaEntry>,
    pub truncated: bool,
}

/// Commit request for atomic update
#[derive(Debug, Clone, Serialize)]
pub struct CommitRequest {
    pub root_hash: String,
    pub expected_generation: u64,
    pub operations: Vec<CommitOp>,
    pub sync_id: String,
}

/// Individual commit operation
#[derive(Debug, Clone, Serialize)]
pub struct CommitOp {
    pub id: String,
    pub hash: String,
    pub op: DeltaOp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// Commit response
#[derive(Debug, Clone, Deserialize)]
pub struct CommitResponse {
    pub success: bool,
    pub new_generation: u64,
    pub new_root_hash: String,
    #[serde(default)]
    pub conflicts: Vec<ConflictEntry>,
}

/// Conflict entry
#[derive(Debug, Clone, Deserialize)]
pub struct ConflictEntry {
    pub id: String,
    pub local_generation: u64,
    pub server_generation: u64,
    pub resolution: Option<String>,
}

/// V4 document entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V4DocEntry {
    pub hash: String,
    #[serde(rename = "type")]
    pub entry_type: String,
    pub uuid: String,
    pub version: String,
    pub size: String,
    pub generation: u64,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

impl From<V4DocEntry> for DocumentInfo {
    fn from(entry: V4DocEntry) -> Self {
        let mut metadata = HashMap::new();
        metadata.insert("generation".into(), serde_json::Value::Number(entry.generation.into()));
        
        Self {
            id: entry.uuid,
            doc_type: entry.entry_type,
            name: entry.name,
            parent: entry.parent,
            hash: Some(entry.hash),
            version: entry.version.parse().unwrap_or(1),
            modified_at: None,
            deleted: false,
            metadata,
        }
    }
}

/// V4 upload context with delta tracking
#[derive(Debug, Clone)]
pub struct V4UploadContext {
    pub sync_id: String,
    pub batch_number: u32,
    pub parent_hash: String,
    pub expected_generation: u64,
    pub operations: Vec<CommitOp>,
}

impl V4UploadContext {
    pub fn new(parent_hash: String, generation: u64) -> Self {
        Self {
            sync_id: Uuid::new_v4().to_string(),
            batch_number: 0,
            parent_hash,
            expected_generation: generation,
            operations: Vec::new(),
        }
    }
    
    pub fn next_batch(&mut self) -> u32 {
        let current = self.batch_number;
        self.batch_number += 1;
        current
    }
    
    pub fn add_operation(&mut self, op: CommitOp) {
        self.operations.push(op);
    }
}

/// V4 Sync Protocol Client
pub struct SyncV4Client {
    client: Client,
    config: SyncConfig,
    last_generation: u64,
}

impl SyncV4Client {
    /// Create a new V4 client
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
        
        Ok(Self {
            client,
            config,
            last_generation: 0,
        })
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
    
    /// Get V4 root
    pub async fn get_v4_root(&self) -> Result<V4Root, SyncError> {
        let url = format!("{}/sync/v4/root", self.sync_url());
        
        let resp = self.client
            .get(&url)
            .header(header::AUTHORIZATION, self.auth_header()?)
            .send()
            .await?;
        
        match resp.status().as_u16() {
            200 => Ok(resp.json().await?),
            401 => Err(SyncError::TokenExpired),
            404 => {
                // V4 not supported, fallback indicator
                Err(SyncError::NotFound("V4 endpoint not available".into()))
            }
            429 => Err(SyncError::RateLimited),
            status => Err(SyncError::Server {
                status,
                message: resp.text().await.unwrap_or_default(),
            }),
        }
    }
    
    /// Get delta changes since a generation
    pub async fn get_delta(&self, from_generation: u64) -> Result<DeltaResponse, SyncError> {
        let url = format!("{}/sync/v4/delta/{}", self.sync_url(), from_generation);
        
        let resp = self.client
            .get(&url)
            .header(header::AUTHORIZATION, self.auth_header()?)
            .send()
            .await?;
        
        match resp.status().as_u16() {
            200 => Ok(resp.json().await?),
            401 => Err(SyncError::TokenExpired),
            404 => Err(SyncError::NotFound("Delta not available".into())),
            429 => Err(SyncError::RateLimited),
            status => Err(SyncError::Server {
                status,
                message: resp.text().await.unwrap_or_default(),
            }),
        }
    }
    
    /// Download file by hash
    pub async fn download_v4_file(&self, hash: &str, rm_filename: &str) -> Result<Vec<u8>, SyncError> {
        if rm_filename.is_empty() {
            return Err(SyncError::MissingFilename);
        }
        
        let filename = rm_filename.split('/').last().unwrap_or(rm_filename);
        let url = format!("{}/sync/v4/files/{}", self.sync_url(), hash);
        
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
    
    /// Upload file with V4 headers
    pub async fn upload_v4_file(
        &self,
        data: &[u8],
        rm_filename: &str,
        ctx: Option<&mut V4UploadContext>,
    ) -> Result<String, SyncError> {
        if rm_filename.is_empty() {
            return Err(SyncError::MissingFilename);
        }
        
        // Calculate SHA-256 hash
        let mut hasher = Sha256::new();
        hasher.update(data);
        let hash = hex::encode(hasher.finalize());
        
        let crc_header = x_goog_hash_header(data);
        let filename = rm_filename.split('/').last().unwrap_or(rm_filename);
        let url = format!("{}/sync/v4/files/{}", self.sync_url(), hash);
        
        let mut req = self.client
            .put(&url)
            .header(header::AUTHORIZATION, self.auth_header()?)
            .header("rm-filename", filename)
            .header("x-goog-hash", crc_header)
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .header(header::CONTENT_LENGTH, data.len());
        
        let expected_gen = ctx.as_ref().map_or(0, |c| c.expected_generation);
        
        if let Some(context) = ctx {
            req = req
                .header("rm-parent-hash", &context.parent_hash)
                .header("rm-sync-id", &context.sync_id)
                .header("rm-batch-number", context.next_batch().to_string())
                .header("rm-expect-version", context.expected_generation.to_string());
        }
        
        let resp = req.body(data.to_vec()).send().await?;
        
        match resp.status().as_u16() {
            200 | 201 => Ok(hash),
            401 => Err(SyncError::TokenExpired),
            409 => Err(SyncError::Conflict {
                local_gen: expected_gen,
                remote_gen: 0,
            }),
            429 => Err(SyncError::RateLimited),
            status => Err(SyncError::Server {
                status,
                message: resp.text().await.unwrap_or_default(),
            }),
        }
    }
    
    /// Commit changes atomically
    pub async fn commit(&self, ctx: &V4UploadContext, new_root_hash: &str) -> Result<CommitResponse, SyncError> {
        let url = format!("{}/sync/v4/commit", self.sync_url());
        
        let request = CommitRequest {
            root_hash: new_root_hash.to_string(),
            expected_generation: ctx.expected_generation,
            operations: ctx.operations.clone(),
            sync_id: ctx.sync_id.clone(),
        };
        
        let resp = self.client
            .post(&url)
            .header(header::AUTHORIZATION, self.auth_header()?)
            .json(&request)
            .send()
            .await?;
        
        match resp.status().as_u16() {
            200 => Ok(resp.json().await?),
            401 => Err(SyncError::TokenExpired),
            409 => {
                // Return the conflict response
                let commit_resp: CommitResponse = resp.json().await?;
                Ok(commit_resp)
            }
            429 => Err(SyncError::RateLimited),
            status => Err(SyncError::Server {
                status,
                message: resp.text().await.unwrap_or_default(),
            }),
        }
    }
    
    /// List documents with generation info
    pub async fn list_v4_docs(&self) -> Result<Vec<V4DocEntry>, SyncError> {
        let root = self.get_v4_root().await?;
        let root_data = self.download_v4_file(&root.hash, "root.docSchema").await?;
        let docs: Vec<V4DocEntry> = serde_json::from_slice(&root_data)
            .map_err(|e| SyncError::Parse(e.to_string()))?;
        Ok(docs)
    }
}

impl SyncProtocol for SyncV4Client {
    fn version(&self) -> SyncVersion {
        SyncVersion::V4
    }
    
    fn base_url(&self) -> &str {
        &self.config.base_url
    }
    
    fn is_authenticated(&self) -> bool {
        self.config.user_token.is_some() || self.config.device_token.is_some()
    }
    
    fn get_root(&self) -> BoxFuture<'_, Result<RootInfo, SyncError>> {
        Box::pin(async move {
            let root = self.get_v4_root().await?;
            Ok(root.into())
        })
    }
    
    fn list_documents(&self) -> BoxFuture<'_, Result<Vec<DocumentInfo>, SyncError>> {
        Box::pin(async move {
            let docs = self.list_v4_docs().await?;
            Ok(docs.into_iter().map(DocumentInfo::from).collect())
        })
    }
    
    fn download_document(&self, id: &str) -> BoxFuture<'_, Result<Document, SyncError>> {
        let id = id.to_string();
        Box::pin(async move {
            // Similar to V3 but with generation tracking
            let docs = self.list_v4_docs().await?;
            let doc_entry = docs.iter()
                .find(|d| d.uuid == id)
                .ok_or_else(|| SyncError::NotFound(id.clone()))?;
            
            // Download document files
            let schema_filename = format!("{}.docSchema", id);
            let schema_data = self.download_v4_file(&doc_entry.hash, &schema_filename).await?;
            
            // Parse schema (V3 format)
            use crate::protocol::v3::V3Schema;
            let schema_str = String::from_utf8_lossy(&schema_data);
            let schema = V3Schema::parse(&schema_str)?;
            
            let mut doc = Document::new(id.clone());
            doc.info = DocumentInfo::from(doc_entry.clone());
            
            for file in &schema.files {
                let data = self.download_v4_file(&file.hash, &file.filename).await?;
                
                if file.filename.ends_with(".rm") {
                    if let Some(page_part) = file.filename.split('/').last() {
                        if let Some(page_id) = page_part.strip_suffix(".rm") {
                            doc.pages.insert(page_id.to_string(), data);
                        }
                    }
                } else {
                    doc.files.insert(file.filename.clone(), data);
                }
            }
            
            Ok(doc)
        })
    }
    
    fn upload_document(&self, doc: &Document) -> BoxFuture<'_, Result<UploadResult, SyncError>> {
        let doc = doc.clone();
        Box::pin(async move {
            let root = self.get_v4_root().await?;
            let mut ctx = V4UploadContext::new(root.hash.clone(), root.generation);
            
            let mut total_size: u64 = 0;
            let mut file_hash = String::new();
            
            // Upload all files
            for (filename, data) in &doc.files {
                file_hash = self.upload_v4_file(data, filename, Some(&mut ctx)).await?;
                total_size += data.len() as u64;
            }
            
            // Upload pages
            for (page_id, data) in &doc.pages {
                let filename = format!("{}.rm", page_id);
                self.upload_v4_file(data, &filename, Some(&mut ctx)).await?;
                total_size += data.len() as u64;
            }
            
            // Add commit operation
            ctx.add_operation(CommitOp {
                id: doc.id.clone(),
                hash: file_hash.clone(),
                op: DeltaOp::Update,
                parent: doc.info.parent.clone(),
                metadata: None,
            });
            
            // Commit changes
            let commit_result = self.commit(&ctx, &file_hash).await?;
            
            if !commit_result.success {
                return Err(SyncError::Conflict {
                    local_gen: ctx.expected_generation,
                    remote_gen: commit_result.new_generation,
                });
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
        let id = id.to_string();
        Box::pin(async move {
            let root = self.get_v4_root().await?;
            let mut ctx = V4UploadContext::new(root.hash.clone(), root.generation);
            
            // Add delete operation
            ctx.add_operation(CommitOp {
                id: id.clone(),
                hash: String::new(),
                op: DeltaOp::Delete,
                parent: None,
                metadata: None,
            });
            
            // Commit delete
            let commit_result = self.commit(&ctx, &root.hash).await?;
            
            if !commit_result.success {
                return Err(SyncError::Conflict {
                    local_gen: ctx.expected_generation,
                    remote_gen: commit_result.new_generation,
                });
            }
            
            Ok(())
        })
    }
    
    fn sync(&self) -> BoxFuture<'_, Result<SyncResult, SyncError>> {
        Box::pin(async move {
            let root = self.get_v4_root().await?;
            
            // Get delta if we have a previous generation
            let delta = if self.last_generation > 0 && self.last_generation < root.generation {
                self.get_delta(self.last_generation).await.ok()
            } else {
                None
            };
            
            let mut result = SyncResult::empty();
            result.generation = root.generation;
            
            if let Some(delta) = delta {
                for entry in delta.entries {
                    match entry.op {
                        DeltaOp::Create | DeltaOp::Update => {
                            result.downloaded.push(entry.id);
                        }
                        DeltaOp::Delete => {
                            result.deleted.push(entry.id);
                        }
                        DeltaOp::Move => {
                            // Treat as update for sync result
                            result.downloaded.push(entry.id);
                        }
                    }
                }
            }
            
            Ok(result)
        })
    }
    
    fn download_file(&self, hash: &str, filename: &str) -> BoxFuture<'_, Result<Vec<u8>, SyncError>> {
        let hash = hash.to_string();
        let filename = filename.to_string();
        Box::pin(async move {
            self.download_v4_file(&hash, &filename).await
        })
    }
    
    fn upload_file(&self, data: &[u8], filename: &str) -> BoxFuture<'_, Result<String, SyncError>> {
        let data = data.to_vec();
        let filename = filename.to_string();
        Box::pin(async move {
            self.upload_v4_file(&data, &filename, None).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_delta_op_serialize() {
        let entry = DeltaEntry {
            id: "test".to_string(),
            hash: Some("abc".to_string()),
            generation: 1,
            op: DeltaOp::Update,
            metadata: None,
        };
        
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("update"));
    }
    
    #[test]
    fn test_v4_doc_entry_conversion() {
        let entry = V4DocEntry {
            hash: "abc123".to_string(),
            entry_type: "DocumentType".to_string(),
            uuid: "test-id".to_string(),
            version: "2".to_string(),
            size: "1000".to_string(),
            generation: 42,
            parent: None,
            name: Some("Test Doc".to_string()),
        };
        
        let info: DocumentInfo = entry.into();
        assert_eq!(info.id, "test-id");
        assert_eq!(info.hash, Some("abc123".to_string()));
        // Generation is stored in metadata
        assert!(info.metadata.contains_key("generation"));
    }
}
