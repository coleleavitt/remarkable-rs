//! Sync V1.5: Transitional protocol with batch operations
//!
//! This transitional protocol (DocumentSync_1_5 from IDA) bridges V1 and V2.
//! It adds batch operations and sync state tracking while maintaining
//! compatibility with the V1 document-storage API.
//!
//! ## Features
//! - Batch document operations
//! - Sync state tracking
//! - Incremental updates
//! - Generation-like version tracking

use std::collections::HashMap;
use reqwest::{Client, header};
use serde::{Deserialize, Serialize};

use crate::error::SyncError;
use crate::protocol::{
    SyncVersion, SyncProtocol, SyncConfig, RootInfo, DocumentInfo,
    Document, SyncResult, UploadResult, BoxFuture, V1DocEntry,
};

/// Sync state for incremental sync
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncState {
    /// Last known version
    pub version: u64,
    /// Document versions by ID
    pub doc_versions: HashMap<String, i32>,
    /// Last sync timestamp
    pub last_sync: Option<String>,
}

impl SyncState {
    /// Create empty sync state
    pub fn new() -> Self {
        Self {
            version: 0,
            doc_versions: HashMap::new(),
            last_sync: None,
        }
    }
}

impl Default for SyncState {
    fn default() -> Self {
        Self::new()
    }
}

/// Batch operation request
#[allow(dead_code)]
#[derive(Debug, Serialize)]
struct BatchRequest {
    operations: Vec<BatchOperation>,
}

/// Individual batch operation
#[derive(Debug, Serialize)]
struct BatchOperation {
    op: String,
    #[serde(rename = "ID")]
    id: String,
    #[serde(rename = "Version")]
    version: i32,
    #[serde(rename = "Type", skip_serializing_if = "Option::is_none")]
    doc_type: Option<String>,
}

/// V1.5 Sync Protocol Client
pub struct SyncV1_5Client {
    client: Client,
    config: SyncConfig,
    state: SyncState,
}

impl SyncV1_5Client {
    /// Create a new V1.5 client
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
            state: SyncState::new(),
        })
    }
    
    /// Create with existing state
    pub fn with_state(mut self, state: SyncState) -> Self {
        self.state = state;
        self
    }
    
    /// Get current sync state
    pub fn state(&self) -> &SyncState {
        &self.state
    }
    
    /// Get authorization header
    fn auth_header(&self) -> Result<String, SyncError> {
        self.config.user_token.as_ref()
            .or(self.config.device_token.as_ref())
            .map(|t| format!("Bearer {}", t))
            .ok_or(SyncError::AuthRequired)
    }
    
    /// Get the document-storage API base URL
    fn api_url(&self) -> String {
        format!("{}/document-storage/json/2", self.config.base_url)
    }
    
    /// List all documents (uses V1 endpoint)
    pub async fn list_docs(&self) -> Result<Vec<V1DocEntry>, SyncError> {
        let url = format!("{}/docs", self.api_url());
        
        let resp = self.client
            .get(&url)
            .header(header::AUTHORIZATION, self.auth_header()?)
            .send()
            .await?;
        
        match resp.status().as_u16() {
            200 => {
                let docs: Vec<V1DocEntry> = resp.json().await?;
                Ok(docs)
            }
            401 => Err(SyncError::TokenExpired),
            404 => Ok(Vec::new()),
            status => Err(SyncError::Server {
                status,
                message: resp.text().await.unwrap_or_default(),
            }),
        }
    }
    
    /// Get changes since last sync
    pub async fn get_changes(&self) -> Result<Vec<V1DocEntry>, SyncError> {
        let docs = self.list_docs().await?;
        
        // Filter to documents that have changed since our last known state
        let changed: Vec<V1DocEntry> = docs.into_iter()
            .filter(|doc| {
                let known_version = self.state.doc_versions.get(&doc.id);
                known_version.map_or(true, |v| doc.version > *v)
            })
            .collect();
        
        Ok(changed)
    }
    
    /// Execute batch operations
    pub async fn batch_upload(&mut self, entries: &[V1DocEntry]) -> Result<Vec<V1DocEntry>, SyncError> {
        let url = format!("{}/upload/request", self.api_url());
        
        let body: Vec<_> = entries.iter().map(|e| BatchOperation {
            op: "upload".to_string(),
            id: e.id.clone(),
            version: e.version,
            doc_type: e.doc_type.clone(),
        }).collect();
        
        let resp = self.client
            .put(&url)
            .header(header::AUTHORIZATION, self.auth_header()?)
            .json(&body)
            .send()
            .await?;
        
        match resp.status().as_u16() {
            200 => {
                let results: Vec<V1DocEntry> = resp.json().await?;
                // Update state with new versions
                for entry in &results {
                    self.state.doc_versions.insert(entry.id.clone(), entry.version);
                }
                self.state.version += 1;
                Ok(results)
            }
            401 => Err(SyncError::TokenExpired),
            status => Err(SyncError::Server {
                status,
                message: resp.text().await.unwrap_or_default(),
            }),
        }
    }
    
    /// Execute batch delete
    pub async fn batch_delete(&mut self, ids: &[(&str, i32)]) -> Result<(), SyncError> {
        let url = format!("{}/delete", self.api_url());
        
        let body: Vec<_> = ids.iter().map(|(id, version)| BatchOperation {
            op: "delete".to_string(),
            id: id.to_string(),
            version: *version,
            doc_type: None,
        }).collect();
        
        let resp = self.client
            .put(&url)
            .header(header::AUTHORIZATION, self.auth_header()?)
            .json(&body)
            .send()
            .await?;
        
        match resp.status().as_u16() {
            200 => {
                // Update state
                for (id, _) in ids {
                    self.state.doc_versions.remove(*id);
                }
                self.state.version += 1;
                Ok(())
            }
            401 => Err(SyncError::TokenExpired),
            status => Err(SyncError::Server {
                status,
                message: resp.text().await.unwrap_or_default(),
            }),
        }
    }
    
    /// Download blob from signed URL
    async fn download_blob(&self, url: &str) -> Result<Vec<u8>, SyncError> {
        let resp = self.client
            .get(url)
            .send()
            .await?;
        
        match resp.status().as_u16() {
            200 => Ok(resp.bytes().await?.to_vec()),
            status => Err(SyncError::Server {
                status,
                message: resp.text().await.unwrap_or_default(),
            }),
        }
    }
    
    /// Upload blob to signed URL
    #[allow(dead_code)]
    async fn upload_blob(&self, url: &str, data: Vec<u8>) -> Result<(), SyncError> {
        let resp = self.client
            .put(url)
            .body(data)
            .send()
            .await?;
        
        match resp.status().as_u16() {
            200 | 201 => Ok(()),
            status => Err(SyncError::Server {
                status,
                message: resp.text().await.unwrap_or_default(),
            }),
        }
    }
}

impl SyncProtocol for SyncV1_5Client {
    fn version(&self) -> SyncVersion {
        SyncVersion::V1_5
    }
    
    fn base_url(&self) -> &str {
        &self.config.base_url
    }
    
    fn is_authenticated(&self) -> bool {
        self.config.user_token.is_some() || self.config.device_token.is_some()
    }
    
    fn get_root(&self) -> BoxFuture<'_, Result<RootInfo, SyncError>> {
        Box::pin(async move {
            Ok(RootInfo::new("v1.5-root".to_string())
                .with_generation(self.state.version))
        })
    }
    
    fn list_documents(&self) -> BoxFuture<'_, Result<Vec<DocumentInfo>, SyncError>> {
        Box::pin(async move {
            let docs = self.list_docs().await?;
            Ok(docs.into_iter().map(DocumentInfo::from).collect())
        })
    }
    
    fn download_document(&self, id: &str) -> BoxFuture<'_, Result<Document, SyncError>> {
        let id = id.to_string();
        Box::pin(async move {
            // Get document entries
            let docs = self.list_docs().await?;
            let entry = docs.iter()
                .find(|d| d.id == id)
                .ok_or_else(|| SyncError::NotFound(id.clone()))?;
            
            let blob_url = entry.blob_url_get.as_ref()
                .ok_or_else(|| SyncError::NotFound(format!("No blob URL for {}", id)))?;
            
            // Download the blob
            let blob_data = self.download_blob(blob_url).await?;
            
            let mut doc = Document::new(id.clone());
            doc.info = DocumentInfo::from(entry.clone());
            doc.files.insert("document.zip".to_string(), blob_data);
            
            Ok(doc)
        })
    }
    
    fn upload_document(&self, doc: &Document) -> BoxFuture<'_, Result<UploadResult, SyncError>> {
        let _doc = doc.clone();
        Box::pin(async move {
            // For V1.5, we need mutable access but trait doesn't allow it
            // This is a limitation - in practice would need interior mutability
            Err(SyncError::NotFound("V1.5 upload requires mutable state".into()))
        })
    }
    
    fn delete_document(&self, id: &str) -> BoxFuture<'_, Result<(), SyncError>> {
        let _id = id.to_string();
        Box::pin(async move {
            // Similar limitation as upload
            Err(SyncError::NotFound("V1.5 delete requires mutable state".into()))
        })
    }
    
    fn sync(&self) -> BoxFuture<'_, Result<SyncResult, SyncError>> {
        Box::pin(async move {
            // Get changes since last sync
            let changes = self.get_changes().await?;
            
            Ok(SyncResult {
                downloaded: changes.iter().map(|d| d.id.clone()).collect(),
                uploaded: Vec::new(),
                deleted: Vec::new(),
                generation: self.state.version,
                had_conflicts: false,
                conflicts: Vec::new(),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_sync_state() {
        let mut state = SyncState::new();
        state.doc_versions.insert("doc1".to_string(), 1);
        state.version = 1;
        
        assert_eq!(state.version, 1);
        assert_eq!(state.doc_versions.get("doc1"), Some(&1));
    }
}
