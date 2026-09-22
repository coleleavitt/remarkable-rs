//! USB Web UI client for reMarkable tablet
//! 
//! Connects to http://10.11.99.1 when device is connected via USB.
//! No authentication required.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum UsbError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Device not connected")]
    NotConnected,
    #[error("Document not found: {0}")]
    NotFound(String),
}

/// Document entry from the USB Web UI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsbDocument {
    #[serde(rename = "ID")]
    pub id: String,
    #[serde(rename = "VissibleName")]
    pub visible_name: String,
    #[serde(rename = "Type")]
    pub doc_type: String,
    #[serde(rename = "Parent")]
    pub parent: String,
    #[serde(rename = "ModifiedClient")]
    pub modified: String,
    #[serde(rename = "CurrentPage")]
    pub current_page: Option<i32>,
    #[serde(rename = "Bookmarked")]
    pub bookmarked: Option<bool>,
}

/// USB Web UI client
pub struct UsbClient {
    client: Client,
    base_url: String,
}

impl UsbClient {
    /// Create a new USB client with default address (10.11.99.1)
    pub fn new() -> Self {
        Self::with_address("10.11.99.1")
    }
    
    /// Create a new USB client with custom address
    pub fn with_address(addr: &str) -> Self {
        Self {
            client: Client::new(),
            base_url: format!("http://{}", addr),
        }
    }
    
    /// Check if device is connected
    pub async fn is_connected(&self) -> bool {
        self.client
            .get(&self.base_url)
            .timeout(std::time::Duration::from_secs(2))
            .send()
            .await
            .is_ok()
    }
    
    /// List all documents
    pub async fn list_documents(&self) -> Result<Vec<UsbDocument>, UsbError> {
        let url = format!("{}/documents/", self.base_url);
        let resp = self.client.get(&url).send().await?;
        
        if !resp.status().is_success() {
            return Err(UsbError::NotConnected);
        }
        
        let docs: Vec<UsbDocument> = resp.json().await?;
        Ok(docs)
    }
    
    /// Download a document by ID
    pub async fn download(&self, doc_id: &str) -> Result<Vec<u8>, UsbError> {
        let url = format!("{}/download/{}/placeholder", self.base_url, doc_id);
        let resp = self.client.get(&url).send().await?;
        
        if resp.status().is_client_error() {
            return Err(UsbError::NotFound(doc_id.to_string()));
        }
        
        let bytes = resp.bytes().await?;
        Ok(bytes.to_vec())
    }
    
    /// Download a document to a file
    pub async fn download_to_file(&self, doc_id: &str, path: &Path) -> Result<(), UsbError> {
        let data = self.download(doc_id).await?;
        std::fs::write(path, data)?;
        Ok(())
    }
    
    /// Upload a document (PDF, EPUB, or zip)
    pub async fn upload(&self, name: &str, data: Vec<u8>) -> Result<(), UsbError> {
        let url = format!("{}/upload", self.base_url);
        
        let part = reqwest::multipart::Part::bytes(data)
            .file_name(name.to_string());
        let form = reqwest::multipart::Form::new()
            .part("file", part);
        
        let resp = self.client.post(&url).multipart(form).send().await?;
        
        if !resp.status().is_success() {
            return Err(UsbError::NotConnected);
        }
        
        Ok(())
    }
    
    /// Upload a file from disk
    pub async fn upload_file(&self, path: &Path) -> Result<(), UsbError> {
        let name = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("document");
        let data = std::fs::read(path)?;
        self.upload(name, data).await
    }
    
    /// Delete a document by ID
    pub async fn delete(&self, doc_id: &str) -> Result<(), UsbError> {
        let url = format!("{}/delete/{}", self.base_url, doc_id);
        let resp = self.client.get(&url).send().await?;
        
        if !resp.status().is_success() {
            return Err(UsbError::NotFound(doc_id.to_string()));
        }
        
        Ok(())
    }
}

impl Default for UsbClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_client_creation() {
        let client = UsbClient::new();
        assert_eq!(client.base_url, "http://10.11.99.1");
    }
    
    #[test]
    fn test_custom_address() {
        let client = UsbClient::with_address("192.168.1.100");
        assert_eq!(client.base_url, "http://192.168.1.100");
    }
}
