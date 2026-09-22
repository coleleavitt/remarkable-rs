//! Device pairing flow integration tests
//!
//! Tests the complete pairing flow:
//! 1. Request pairing code from authenticated endpoint
//! 2. Exchange code for device token
//! 3. Refresh user token
//!
//! # Test modes
//! 
//! - Mock: Uses MockSyncServer for offline testing
//! - Device: Uses real device when available (requires USB connection)

mod common;

use common::{MockSyncServer, fixtures::*};
use remarkable_sync::{SyncClient, SyncError};
use reqwest::Client;
use serde_json::json;

/// Test pairing code exchange with mock server
#[tokio::test]
async fn test_pairing_code_exchange_mock() {
    let server = MockSyncServer::start().await;
    
    // Register a valid pairing code
    server.state().add_pairing_code("testcode", "test-device-001");
    
    let client = Client::new();
    
    // Exchange pairing code for device token
    let resp = client
        .post(format!("{}/token/json/2/device/new", server.base_url()))
        .json(&json!({
            "code": "testcode",
            "deviceDesc": "desktop-linux",
            "deviceID": "test-device-001"
        }))
        .send()
        .await
        .unwrap();
    
    assert_eq!(resp.status(), 200);
    
    let device_token = resp.text().await.unwrap();
    assert!(device_token.contains("mock-device"));
}

/// Test pairing code exchange fails with invalid code
#[tokio::test]
async fn test_pairing_invalid_code_rejected() {
    let server = MockSyncServer::start().await;
    
    let client = Client::new();
    
    let resp = client
        .post(format!("{}/token/json/2/device/new", server.base_url()))
        .json(&json!({
            "code": "badcode",
            "deviceDesc": "desktop-linux",
            "deviceID": "test-device-001"
        }))
        .send()
        .await
        .unwrap();
    
    assert_eq!(resp.status(), 401);
}

/// Test user token refresh with valid device token
#[tokio::test]
async fn test_user_token_refresh_mock() {
    let server = MockSyncServer::start().await;
    
    // Add a device token to valid tokens
    server.state().add_token("device-token-123");
    
    let client = Client::new();
    
    let resp = client
        .post(format!("{}/token/json/2/user/new", server.base_url()))
        .header("Authorization", "Bearer device-token-123")
        .send()
        .await
        .unwrap();
    
    assert_eq!(resp.status(), 200);
    
    let user_token = resp.text().await.unwrap();
    // Should be a JWT-like format
    assert!(user_token.split('.').count() == 3);
}

/// Test user token refresh fails without auth
#[tokio::test]
async fn test_user_token_refresh_requires_auth() {
    let server = MockSyncServer::start().await;
    
    let client = Client::new();
    
    let resp = client
        .post(format!("{}/token/json/2/user/new", server.base_url()))
        .send()
        .await
        .unwrap();
    
    assert_eq!(resp.status(), 401);
}

/// Full pairing flow: code -> device token -> user token
#[tokio::test]
async fn test_full_pairing_flow_mock() {
    let server = MockSyncServer::start().await;
    
    // Setup: Add pairing code
    let device_id = "full-flow-device";
    server.state().add_pairing_code("flowcode", device_id);
    
    let client = Client::new();
    
    // Step 1: Exchange pairing code for device token
    let resp = client
        .post(format!("{}/token/json/2/device/new", server.base_url()))
        .json(&json!({
            "code": "flowcode",
            "deviceDesc": "remarkable",
            "deviceID": device_id
        }))
        .send()
        .await
        .unwrap();
    
    assert_eq!(resp.status(), 200);
    let device_token = resp.text().await.unwrap();
    
    // Step 2: Use device token to get user token
    let resp = client
        .post(format!("{}/token/json/2/user/new", server.base_url()))
        .header("Authorization", format!("Bearer {}", device_token))
        .send()
        .await
        .unwrap();
    
    assert_eq!(resp.status(), 200);
    let user_token = resp.text().await.unwrap();
    
    // Step 3: Verify user token works for sync operations
    let resp = client
        .get(format!("{}/sync/v3/root", server.base_url()))
        .header("Authorization", format!("Bearer {}", user_token))
        .send()
        .await
        .unwrap();
    
    assert_eq!(resp.status(), 200);
}

/// Test with real device (skipped if device not available)
#[tokio::test]
#[ignore = "requires USB-connected device"]
async fn test_device_connection() {
    if !device_available().await {
        eprintln!("Device not available at {}", DEVICE_USB_ADDR);
        return;
    }
    
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();
    
    // Check device web UI is accessible
    let resp = client
        .get(format!("http://{}/documents/", DEVICE_USB_ADDR))
        .send()
        .await
        .unwrap();
    
    assert!(resp.status().is_success());
}

/// Test with captured tokens (skipped if tokens not available)
#[tokio::test]
#[ignore = "requires captured tokens from device"]
async fn test_captured_tokens_valid() {
    let (device_token, user_token) = match load_captured_tokens() {
        Some(tokens) => tokens,
        None => {
            eprintln!("Captured tokens not found in {}", TOKEN_DIR);
            return;
        }
    };
    
    // Verify tokens are non-empty
    assert!(!device_token.is_empty(), "Device token is empty");
    assert!(!user_token.is_empty(), "User token is empty");
    
    // Verify JWT format (3 parts separated by dots)
    assert_eq!(device_token.split('.').count(), 3, "Device token not JWT format");
    assert_eq!(user_token.split('.').count(), 3, "User token not JWT format");
    
    // TODO: Could test actual API call here, but tokens may be expired
}

/// Test mock token generation
#[test]
fn test_mock_token_generation() {
    let device_token = mock_device_token("test-device");
    let user_token = mock_user_token("eu");
    
    // Both should be valid JWT format
    assert_eq!(device_token.split('.').count(), 3);
    assert_eq!(user_token.split('.').count(), 3);
}

/// Test discovery endpoint (no auth required)
#[tokio::test]
async fn test_discovery_endpoint_mock() {
    let server = MockSyncServer::start().await;
    let client = Client::new();
    
    let resp = client
        .get(format!("{}/discovery/v1/endpoints", server.base_url()))
        .send()
        .await
        .unwrap();
    
    assert_eq!(resp.status(), 200);
    
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body.get("Webapp").is_some());
    assert!(body.get("Auth0").is_some());
}
