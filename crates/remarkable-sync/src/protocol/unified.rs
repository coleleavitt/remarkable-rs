//! Unified sync client
//!
//! Provides a single client that can work with any sync protocol version.
//! Automatically detects the server's protocol version or uses a specified one.

use std::sync::Arc;

use crate::error::SyncError;
use crate::protocol::{
    SyncVersion, SyncProtocol, SyncConfig, RootInfo, DocumentInfo,
    Document, SyncResult, UploadResult, BoxFuture,
    SyncV1Client, SyncV1_5Client, SyncV2Client, SyncV3Client, SyncV4Client,
    detect_protocol, ProtocolDetector,
};

/// Unified sync client that works with any protocol version
pub struct UnifiedSyncClient {
    inner: Arc<dyn SyncProtocol>,
    version: SyncVersion,
    config: SyncConfig,
}

impl UnifiedSyncClient {
    /// Create a client with automatic protocol detection
    pub async fn new(config: SyncConfig) -> Result<Self, SyncError> {
        let version = if let Some(v) = config.preferred_version {
            v
        } else {
            detect_protocol(&config).await?
        };
        
        Self::with_version(config, version)
    }
    
    /// Create a client with a specific protocol version
    pub fn with_version(config: SyncConfig, version: SyncVersion) -> Result<Self, SyncError> {
        let inner: Arc<dyn SyncProtocol> = match version {
            SyncVersion::V1 => Arc::new(SyncV1Client::new(config.clone())?),
            SyncVersion::V1_5 => Arc::new(SyncV1_5Client::new(config.clone())?),
            SyncVersion::V2 => Arc::new(SyncV2Client::new(config.clone())?),
            SyncVersion::V3 => Arc::new(SyncV3Client::new(config.clone())?),
            SyncVersion::V4 => Arc::new(SyncV4Client::new(config.clone())?),
        };
        
        Ok(Self {
            inner,
            version,
            config,
        })
    }
    
    /// Create a V1 client
    pub fn v1(config: SyncConfig) -> Result<Self, SyncError> {
        Self::with_version(config, SyncVersion::V1)
    }
    
    /// Create a V1.5 client
    pub fn v1_5(config: SyncConfig) -> Result<Self, SyncError> {
        Self::with_version(config, SyncVersion::V1_5)
    }
    
    /// Create a V2 client
    pub fn v2(config: SyncConfig) -> Result<Self, SyncError> {
        Self::with_version(config, SyncVersion::V2)
    }
    
    /// Create a V3 client
    pub fn v3(config: SyncConfig) -> Result<Self, SyncError> {
        Self::with_version(config, SyncVersion::V3)
    }
    
    /// Create a V4 client
    pub fn v4(config: SyncConfig) -> Result<Self, SyncError> {
        Self::with_version(config, SyncVersion::V4)
    }
    
    /// Get the protocol version in use
    pub fn version(&self) -> SyncVersion {
        self.version
    }
    
    /// Get the underlying protocol implementation
    pub fn protocol(&self) -> &dyn SyncProtocol {
        self.inner.as_ref()
    }
    
    /// Get the configuration
    pub fn config(&self) -> &SyncConfig {
        &self.config
    }
    
    /// Check if connected/authenticated
    pub fn is_authenticated(&self) -> bool {
        self.inner.is_authenticated()
    }
    
    /// Get root information
    pub async fn get_root(&self) -> Result<RootInfo, SyncError> {
        self.inner.get_root().await
    }
    
    /// List all documents
    pub async fn list_documents(&self) -> Result<Vec<DocumentInfo>, SyncError> {
        self.inner.list_documents().await
    }
    
    /// Download a document
    pub async fn download_document(&self, id: &str) -> Result<Document, SyncError> {
        self.inner.download_document(id).await
    }
    
    /// Upload a document
    pub async fn upload_document(&self, doc: &Document) -> Result<UploadResult, SyncError> {
        self.inner.upload_document(doc).await
    }
    
    /// Delete a document
    pub async fn delete_document(&self, id: &str) -> Result<(), SyncError> {
        self.inner.delete_document(id).await
    }
    
    /// Perform full sync
    pub async fn sync(&self) -> Result<SyncResult, SyncError> {
        self.inner.sync().await
    }
    
    /// Download a file by hash (for hash-based protocols)
    pub async fn download_file(&self, hash: &str, filename: &str) -> Result<Vec<u8>, SyncError> {
        self.inner.download_file(hash, filename).await
    }
    
    /// Upload a file (for hash-based protocols)
    pub async fn upload_file(&self, data: &[u8], filename: &str) -> Result<String, SyncError> {
        self.inner.upload_file(data, filename).await
    }
    
    /// Create a folder
    pub async fn create_folder(&self, name: &str, parent: Option<&str>) -> Result<DocumentInfo, SyncError> {
        self.inner.create_folder(name, parent).await
    }
    
    /// Rename a document
    pub async fn rename_document(&self, id: &str, new_name: &str) -> Result<(), SyncError> {
        self.inner.rename_document(id, new_name).await
    }
    
    /// Move a document to a different folder
    pub async fn move_document(&self, id: &str, new_parent: Option<&str>) -> Result<(), SyncError> {
        self.inner.move_document(id, new_parent).await
    }
    
    /// Upgrade to a newer protocol version if available
    pub async fn upgrade(&mut self) -> Result<bool, SyncError> {
        let detector = ProtocolDetector::new(self.config.clone())?;
        let result = detector.detect().await;
        
        if result.success && result.version > self.version {
            let new_inner: Arc<dyn SyncProtocol> = match result.version {
                SyncVersion::V1 => Arc::new(SyncV1Client::new(self.config.clone())?),
                SyncVersion::V1_5 => Arc::new(SyncV1_5Client::new(self.config.clone())?),
                SyncVersion::V2 => Arc::new(SyncV2Client::new(self.config.clone())?),
                SyncVersion::V3 => Arc::new(SyncV3Client::new(self.config.clone())?),
                SyncVersion::V4 => Arc::new(SyncV4Client::new(self.config.clone())?),
            };
            
            self.inner = new_inner;
            self.version = result.version;
            Ok(true)
        } else {
            Ok(false)
        }
    }
    
    /// Downgrade to a specific protocol version
    pub fn downgrade(&mut self, version: SyncVersion) -> Result<(), SyncError> {
        if version < self.version {
            let new_inner: Arc<dyn SyncProtocol> = match version {
                SyncVersion::V1 => Arc::new(SyncV1Client::new(self.config.clone())?),
                SyncVersion::V1_5 => Arc::new(SyncV1_5Client::new(self.config.clone())?),
                SyncVersion::V2 => Arc::new(SyncV2Client::new(self.config.clone())?),
                SyncVersion::V3 => Arc::new(SyncV3Client::new(self.config.clone())?),
                SyncVersion::V4 => Arc::new(SyncV4Client::new(self.config.clone())?),
            };
            
            self.inner = new_inner;
            self.version = version;
        }
        Ok(())
    }
}

/// Builder for UnifiedSyncClient
pub struct UnifiedClientBuilder {
    config: SyncConfig,
    version: Option<SyncVersion>,
    auto_detect: bool,
}

impl UnifiedClientBuilder {
    /// Create a new builder with cloud config
    pub fn cloud() -> Self {
        Self {
            config: SyncConfig::cloud(),
            version: None,
            auto_detect: true,
        }
    }
    
    /// Create a new builder with local server config
    pub fn local(url: impl Into<String>) -> Self {
        Self {
            config: SyncConfig::local(url),
            version: None,
            auto_detect: true,
        }
    }
    
    /// Set device token
    pub fn device_token(mut self, token: impl Into<String>) -> Self {
        self.config.device_token = Some(token.into());
        self
    }
    
    /// Set user token
    pub fn user_token(mut self, token: impl Into<String>) -> Self {
        self.config.user_token = Some(token.into());
        self
    }
    
    /// Set region
    pub fn region(mut self, region: impl Into<String>) -> Self {
        self.config.region = Some(region.into());
        self
    }
    
    /// Set specific protocol version (disables auto-detection)
    pub fn version(mut self, version: SyncVersion) -> Self {
        self.version = Some(version);
        self.auto_detect = false;
        self
    }
    
    /// Enable auto-detection (default)
    pub fn auto_detect(mut self) -> Self {
        self.auto_detect = true;
        self.version = None;
        self
    }
    
    /// Skip TLS verification
    pub fn skip_tls_verify(mut self) -> Self {
        self.config.skip_tls_verify = true;
        self
    }
    
    /// Set request timeout
    pub fn timeout(mut self, secs: u64) -> Self {
        self.config.timeout_secs = secs;
        self
    }
    
    /// Build the client
    pub async fn build(self) -> Result<UnifiedSyncClient, SyncError> {
        if let Some(version) = self.version {
            UnifiedSyncClient::with_version(self.config, version)
        } else if self.auto_detect {
            UnifiedSyncClient::new(self.config).await
        } else {
            UnifiedSyncClient::with_version(self.config, SyncVersion::V3)
        }
    }
    
    /// Build with a specific version (synchronous)
    pub fn build_sync(self, version: SyncVersion) -> Result<UnifiedSyncClient, SyncError> {
        UnifiedSyncClient::with_version(self.config, version)
    }
}

/// Implement SyncProtocol for UnifiedSyncClient
impl SyncProtocol for UnifiedSyncClient {
    fn version(&self) -> SyncVersion {
        self.version
    }
    
    fn base_url(&self) -> &str {
        self.inner.base_url()
    }
    
    fn is_authenticated(&self) -> bool {
        self.inner.is_authenticated()
    }
    
    fn get_root(&self) -> BoxFuture<'_, Result<RootInfo, SyncError>> {
        Box::pin(async move {
            self.inner.get_root().await
        })
    }
    
    fn list_documents(&self) -> BoxFuture<'_, Result<Vec<DocumentInfo>, SyncError>> {
        Box::pin(async move {
            self.inner.list_documents().await
        })
    }
    
    fn download_document(&self, id: &str) -> BoxFuture<'_, Result<Document, SyncError>> {
        let id = id.to_string();
        Box::pin(async move {
            self.inner.download_document(&id).await
        })
    }
    
    fn upload_document(&self, doc: &Document) -> BoxFuture<'_, Result<UploadResult, SyncError>> {
        let doc = doc.clone();
        Box::pin(async move {
            self.inner.upload_document(&doc).await
        })
    }
    
    fn delete_document(&self, id: &str) -> BoxFuture<'_, Result<(), SyncError>> {
        let id = id.to_string();
        Box::pin(async move {
            self.inner.delete_document(&id).await
        })
    }
    
    fn sync(&self) -> BoxFuture<'_, Result<SyncResult, SyncError>> {
        Box::pin(async move {
            self.inner.sync().await
        })
    }
    
    fn download_file(&self, hash: &str, filename: &str) -> BoxFuture<'_, Result<Vec<u8>, SyncError>> {
        let hash = hash.to_string();
        let filename = filename.to_string();
        Box::pin(async move {
            self.inner.download_file(&hash, &filename).await
        })
    }
    
    fn upload_file(&self, data: &[u8], filename: &str) -> BoxFuture<'_, Result<String, SyncError>> {
        let data = data.to_vec();
        let filename = filename.to_string();
        Box::pin(async move {
            self.inner.upload_file(&data, &filename).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_builder_sync() {
        let client = UnifiedClientBuilder::cloud()
            .version(SyncVersion::V3)
            .build_sync(SyncVersion::V3);
        
        assert!(client.is_ok());
        assert_eq!(client.unwrap().version(), SyncVersion::V3);
    }
    
    #[test]
    fn test_version_comparison() {
        assert!(SyncVersion::V4 > SyncVersion::V3);
        assert!(SyncVersion::V3 > SyncVersion::V2);
        assert!(SyncVersion::V2 > SyncVersion::V1_5);
        assert!(SyncVersion::V1_5 > SyncVersion::V1);
    }
}
