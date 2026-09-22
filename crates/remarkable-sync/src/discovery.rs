//! Service discovery client

use serde::{Deserialize, Serialize};
use crate::SyncError;

/// Discovery response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryResponse {
    #[serde(rename = "Host")]
    pub host: String,
    #[serde(rename = "Status")]
    pub status: String,
}

/// Service discovery client
pub struct DiscoveryClient {
    client: reqwest::Client,
    base_url: String,
}

impl DiscoveryClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: crate::DISCOVERY_URL.to_string(),
        }
    }
    
    /// Discover service endpoints
    pub async fn discover(&self) -> Result<Endpoints, SyncError> {
        let resp: DiscoveryResponse = self.client
            .get(format!("{}/service/json/1/document-storage", self.base_url))
            .query(&[("environment", "production"), ("group", "auth0|user"), ("apiVer", "2")])
            .send()
            .await?
            .json()
            .await?;
        
        Ok(Endpoints {
            storage: resp.host,
            notifications: "vernemq-prod.cloud.remarkable.engineering".to_string(),
            webapp: "my.remarkable.com".to_string(),
        })
    }
}

impl Default for DiscoveryClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Discovered service endpoints
#[derive(Debug, Clone)]
pub struct Endpoints {
    pub storage: String,
    pub notifications: String,
    pub webapp: String,
}
