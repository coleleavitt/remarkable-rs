//! Mock sync server for offline testing
//!
//! Implements a subset of the reMarkable cloud API for local testing.
//! Start with `MockSyncServer::start()` and use `base_url()` as the endpoint.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::net::SocketAddr;
use tokio::sync::oneshot;
use axum::{
    Router,
    routing::{get, post, put},
    extract::{Path, State},
    http::{StatusCode, HeaderMap},
    response::{IntoResponse, Response},
    Json,
    body::Bytes,
};
use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};
use base64::Engine;

/// Mock sync server state
#[derive(Clone)]
pub struct MockState {
    /// Stored files: hash -> (filename, data)
    files: Arc<Mutex<HashMap<String, (String, Vec<u8>)>>>,
    /// Root hash
    root_hash: Arc<Mutex<String>>,
    /// Generation counter
    generation: Arc<Mutex<u64>>,
    /// Document entries
    documents: Arc<Mutex<Vec<MockDocEntry>>>,
    /// Valid tokens for auth checking
    valid_tokens: Arc<Mutex<Vec<String>>>,
    /// One-time pairing codes: code -> device_id  
    pairing_codes: Arc<Mutex<HashMap<String, String>>>,
    /// Device tokens issued
    device_tokens: Arc<Mutex<HashMap<String, String>>>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct MockDocEntry {
    pub hash: String,
    #[serde(rename = "type")]
    pub entry_type: String,
    pub uuid: String,
    pub version: String,
    pub size: String,
}

impl MockState {
    pub fn new() -> Self {
        Self {
            files: Arc::new(Mutex::new(HashMap::new())),
            root_hash: Arc::new(Mutex::new(String::new())),
            generation: Arc::new(Mutex::new(1)),
            documents: Arc::new(Mutex::new(Vec::new())),
            valid_tokens: Arc::new(Mutex::new(vec!["test-token".to_string()])),
            pairing_codes: Arc::new(Mutex::new(HashMap::new())),
            device_tokens: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    
    /// Add a valid auth token
    pub fn add_token(&self, token: &str) {
        self.valid_tokens.lock().unwrap().push(token.to_string());
    }
    
    /// Add a pairing code that can be exchanged for a device token
    pub fn add_pairing_code(&self, code: &str, device_id: &str) {
        self.pairing_codes.lock().unwrap().insert(code.to_string(), device_id.to_string());
    }
    
    /// Add a document to the mock store
    pub fn add_document(&self, uuid: &str, metadata: &[u8], content: &[u8], pages: Vec<(&str, &[u8])>) {
        let mut files = self.files.lock().unwrap();
        let mut docs = self.documents.lock().unwrap();
        
        // Hash and store metadata
        let metadata_hash = hash_content(metadata);
        let metadata_filename = format!("{}.metadata", uuid);
        files.insert(metadata_hash.clone(), (metadata_filename.clone(), metadata.to_vec()));
        
        // Hash and store content
        let content_hash = hash_content(content);
        let content_filename = format!("{}.content", uuid);
        files.insert(content_hash.clone(), (content_filename.clone(), content.to_vec()));
        
        // Build schema.txt
        let mut schema_lines = vec![format!("{}", 2 + pages.len())];
        schema_lines.push(format!("{}:0:{}:0:{}", metadata_hash, metadata_filename, metadata.len()));
        schema_lines.push(format!("{}:0:{}:0:{}", content_hash, content_filename, content.len()));
        
        for (page_id, page_data) in &pages {
            let page_hash = hash_content(page_data);
            let page_filename = format!("{}/{}.rm", uuid, page_id);
            files.insert(page_hash.clone(), (page_filename.clone(), page_data.to_vec()));
            schema_lines.push(format!("{}:0:{}:0:{}", page_hash, page_filename, page_data.len()));
        }
        
        let schema = schema_lines.join("\n");
        let schema_hash = hash_content(schema.as_bytes());
        let schema_filename = format!("{}/schema.txt", uuid);
        files.insert(schema_hash.clone(), (schema_filename, schema.as_bytes().to_vec()));
        
        // Create doc entry pointing to schema
        let doc_entry = MockDocEntry {
            hash: schema_hash.clone(),
            entry_type: "DocumentType".to_string(),
            uuid: uuid.to_string(),
            version: "1".to_string(),
            size: schema.len().to_string(),
        };
        docs.push(doc_entry);
        
        // Update root index
        let root_json = serde_json::to_vec(&*docs).unwrap();
        let new_root_hash = hash_content(&root_json);
        files.insert(new_root_hash.clone(), ("root.docSchema".to_string(), root_json));
        
        drop(files);
        drop(docs);
        
        *self.root_hash.lock().unwrap() = new_root_hash;
        *self.generation.lock().unwrap() += 1;
    }
    
    fn check_auth(&self, headers: &HeaderMap) -> bool {
        if let Some(auth) = headers.get("authorization") {
            if let Ok(auth_str) = auth.to_str() {
                if let Some(token) = auth_str.strip_prefix("Bearer ") {
                    return self.valid_tokens.lock().unwrap().contains(&token.to_string());
                }
            }
        }
        false
    }
}

fn hash_content(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// Mock sync server
pub struct MockSyncServer {
    addr: SocketAddr,
    shutdown_tx: Option<oneshot::Sender<()>>,
    state: MockState,
}

impl MockSyncServer {
    /// Start the mock server on a random port
    pub async fn start() -> Self {
        Self::start_with_state(MockState::new()).await
    }
    
    /// Start with custom state
    pub async fn start_with_state(state: MockState) -> Self {
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        
        let app = Router::new()
            // Sync v3 endpoints
            .route("/sync/v3/root", get(handle_root))
            .route("/sync/v3/files/{hash}", get(handle_download).put(handle_upload))
            // Auth endpoints
            .route("/token/json/2/device/new", post(handle_device_new))
            .route("/token/json/2/user/new", post(handle_user_new))
            // Discovery
            .route("/discovery/v1/endpoints", get(handle_discovery))
            .with_state(state.clone());
        
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        
        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .unwrap();
        });
        
        Self {
            addr,
            shutdown_tx: Some(shutdown_tx),
            state,
        }
    }
    
    /// Get the base URL for this mock server
    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }
    
    /// Get reference to state for setup
    pub fn state(&self) -> &MockState {
        &self.state
    }
    
    /// Stop the server
    pub fn stop(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

impl Drop for MockSyncServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

// Handler implementations

#[derive(Serialize)]
struct RootResponse {
    hash: String,
    generation: u64,
}

async fn handle_root(
    State(state): State<MockState>,
    headers: HeaderMap,
) -> Response {
    if !state.check_auth(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    
    let hash = state.root_hash.lock().unwrap().clone();
    let generation = *state.generation.lock().unwrap();
    
    Json(RootResponse { hash, generation }).into_response()
}

async fn handle_download(
    State(state): State<MockState>,
    Path(hash): Path<String>,
    headers: HeaderMap,
) -> Response {
    if !state.check_auth(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    
    // Check rm-filename header is present
    if headers.get("rm-filename").is_none() {
        return (
            StatusCode::BAD_REQUEST,
            "unexpected 'rm-filename' http header"
        ).into_response();
    }
    
    let files = state.files.lock().unwrap();
    if let Some((_filename, data)) = files.get(&hash) {
        // Calculate CRC32C for response header
        let crc = crc32c::crc32c(data);
        let crc_bytes = crc.to_be_bytes();
        let crc_b64 = base64::engine::general_purpose::STANDARD.encode(&crc_bytes);
        
        let mut response = data.clone().into_response();
        response.headers_mut().insert(
            "x-goog-hash",
            format!("crc32c={}", crc_b64).parse().unwrap(),
        );
        response
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

async fn handle_upload(
    State(state): State<MockState>,
    Path(expected_hash): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !state.check_auth(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    
    // Get rm-filename header
    let filename = match headers.get("rm-filename") {
        Some(f) => f.to_str().unwrap_or("unknown").to_string(),
        None => return (
            StatusCode::BAD_REQUEST,
            "Missing rm-filename header"
        ).into_response(),
    };
    
    // Verify hash matches
    let actual_hash = hash_content(&body);
    if actual_hash != expected_hash {
        return (
            StatusCode::BAD_REQUEST,
            format!("Hash mismatch: expected {}, got {}", expected_hash, actual_hash),
        ).into_response();
    }
    
    // Store the file
    let mut files = state.files.lock().unwrap();
    files.insert(actual_hash, (filename, body.to_vec()));
    
    // Increment generation
    *state.generation.lock().unwrap() += 1;
    
    StatusCode::OK.into_response()
}

#[derive(Deserialize)]
struct DeviceNewRequest {
    code: String,
    #[serde(rename = "deviceDesc")]
    device_desc: String,
    #[serde(rename = "deviceID")]
    device_id: String,
}

async fn handle_device_new(
    State(state): State<MockState>,
    Json(req): Json<DeviceNewRequest>,
) -> Response {
    let codes = state.pairing_codes.lock().unwrap();
    
    if codes.get(&req.code).is_some() {
        // Generate a mock device token (JWT-like format)
        let device_token = format!(
            "eyJhbGciOiJIUzI1NiJ9.{{}}.mock-device-{}",
            req.device_id
        );
        
        // Store the device token
        state.device_tokens.lock().unwrap().insert(req.device_id.clone(), device_token.clone());
        state.valid_tokens.lock().unwrap().push(device_token.clone());
        
        device_token.into_response()
    } else {
        StatusCode::UNAUTHORIZED.into_response()
    }
}

async fn handle_user_new(
    State(state): State<MockState>,
    headers: HeaderMap,
) -> Response {
    if !state.check_auth(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    
    // Generate a mock user token with claims
    let claims = serde_json::json!({
        "sub": "test-user-123",
        "https://auth.remarkable.com/tectonic": "mock",
        "scopes": "sync:fox intgr hwc screenshare"
    });
    let claims_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(claims.to_string().as_bytes());
    
    let user_token = format!(
        "eyJhbGciOiJIUzI1NiJ9.{}.mock-user-sig",
        claims_b64
    );
    
    state.valid_tokens.lock().unwrap().push(user_token.clone());
    user_token.into_response()
}

#[derive(Serialize)]
struct DiscoveryResponse {
    #[serde(rename = "Webapp")]
    webapp: String,
    #[serde(rename = "Auth0")]
    auth0: String,
    #[serde(rename = "SyncWS")]
    sync_ws: String,
}

async fn handle_discovery() -> Json<DiscoveryResponse> {
    // Discovery doesn't require auth
    Json(DiscoveryResponse {
        webapp: "http://localhost/webapp".to_string(),
        auth0: "http://localhost/auth0".to_string(),
        sync_ws: "ws://localhost/sync".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_mock_server_starts() {
        let server = MockSyncServer::start().await;
        let url = server.base_url();
        assert!(url.starts_with("http://127.0.0.1:"));
    }
    
    #[tokio::test]
    async fn test_discovery_no_auth() {
        let server = MockSyncServer::start().await;
        let client = reqwest::Client::new();
        
        let resp = client
            .get(format!("{}/discovery/v1/endpoints", server.base_url()))
            .send()
            .await
            .unwrap();
        
        assert_eq!(resp.status(), 200);
    }
    
    #[tokio::test]
    async fn test_root_requires_auth() {
        let server = MockSyncServer::start().await;
        let client = reqwest::Client::new();
        
        let resp = client
            .get(format!("{}/sync/v3/root", server.base_url()))
            .send()
            .await
            .unwrap();
        
        assert_eq!(resp.status(), 401);
    }
    
    #[tokio::test]
    async fn test_root_with_auth() {
        let server = MockSyncServer::start().await;
        let client = reqwest::Client::new();
        
        let resp = client
            .get(format!("{}/sync/v3/root", server.base_url()))
            .header("Authorization", "Bearer test-token")
            .send()
            .await
            .unwrap();
        
        assert_eq!(resp.status(), 200);
    }
}
