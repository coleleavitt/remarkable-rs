//! GraphQL client for share.remarkable.com
//!
//! Query shared documents and their metadata.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum GraphqlError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("GraphQL error: {0}")]
    Graphql(String),
    #[error("Document not found")]
    NotFound,
}

/// GraphQL client
pub struct ShareClient {
    client: Client,
    endpoint: String,
}

impl ShareClient {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            endpoint: "https://share.remarkable.com/api/graphql".to_string(),
        }
    }
    
    /// Execute a GraphQL query
    pub async fn query<T: for<'de> Deserialize<'de>>(
        &self,
        query: &str,
        variables: Option<serde_json::Value>,
    ) -> Result<T, GraphqlError> {
        let body = serde_json::json!({
            "query": query,
            "variables": variables.unwrap_or(serde_json::Value::Null)
        });
        
        let resp = self.client
            .post(&self.endpoint)
            .json(&body)
            .send()
            .await?;
        
        let data: GraphqlResponse<T> = resp.json().await?;
        
        if let Some(errors) = data.errors {
            if !errors.is_empty() {
                return Err(GraphqlError::Graphql(
                    errors.into_iter().map(|e| e.message).collect::<Vec<_>>().join(", ")
                ));
            }
        }
        
        data.data.ok_or(GraphqlError::NotFound)
    }
    
    /// Get shared document by ID
    pub async fn get_document(&self, doc_id: &str) -> Result<SharedDocument, GraphqlError> {
        let query = r#"
            query GetDocument($id: ID!) {
                document(id: $id) {
                    id
                    name
                    pageCount
                    createdAt
                    modifiedAt
                    thumbnailUrl
                }
            }
        "#;
        
        let vars = serde_json::json!({ "id": doc_id });
        
        #[derive(Deserialize)]
        struct Response {
            document: SharedDocument,
        }
        
        let resp: Response = self.query(query, Some(vars)).await?;
        Ok(resp.document)
    }
    
    /// Get document pages
    pub async fn get_pages(&self, doc_id: &str) -> Result<Vec<SharedPage>, GraphqlError> {
        let query = r#"
            query GetPages($id: ID!) {
                document(id: $id) {
                    pages {
                        id
                        pageNumber
                        imageUrl
                    }
                }
            }
        "#;
        
        let vars = serde_json::json!({ "id": doc_id });
        
        #[derive(Deserialize)]
        struct Document {
            pages: Vec<SharedPage>,
        }
        
        #[derive(Deserialize)]
        struct Response {
            document: Document,
        }
        
        let resp: Response = self.query(query, Some(vars)).await?;
        Ok(resp.document.pages)
    }
}

impl Default for ShareClient {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct GraphqlResponse<T> {
    data: Option<T>,
    errors: Option<Vec<GraphqlErrorItem>>,
}

#[derive(Debug, Deserialize)]
struct GraphqlErrorItem {
    message: String,
}

/// A shared document
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedDocument {
    pub id: String,
    pub name: String,
    #[serde(rename = "pageCount")]
    pub page_count: i32,
    #[serde(rename = "createdAt")]
    pub created_at: Option<String>,
    #[serde(rename = "modifiedAt")]
    pub modified_at: Option<String>,
    #[serde(rename = "thumbnailUrl")]
    pub thumbnail_url: Option<String>,
}

/// A page in a shared document
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedPage {
    pub id: String,
    #[serde(rename = "pageNumber")]
    pub page_number: i32,
    #[serde(rename = "imageUrl")]
    pub image_url: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_client_creation() {
        let client = ShareClient::new();
        assert!(client.endpoint.contains("graphql"));
    }
}
