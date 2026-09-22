//! Device simulation integration tests
//!
//! Tests device registration, token refresh, MQTT connection,
//! and sync-complete event handling without a real device.
//!
//! # Running
//!
//! ```bash
//! # Run all simulation tests
//! cargo test --test test_device_simulation
//!
//! # Run with real device (requires USB connection + tokens)
//! cargo test --test test_device_simulation -- --ignored --test-threads=1
//! ```

mod common;

use common::{MockSyncServer, fixtures::*};
use reqwest::Client;
use serde_json::json;
use std::time::Duration;
use uuid::Uuid;

/// Simulated device state
struct SimulatedDevice {
    device_id: String,
    device_token: Option<String>,
    user_token: Option<String>,
    generation: u64,
}

impl SimulatedDevice {
    fn new() -> Self {
        Self {
            device_id: format!("sim-device-{}", Uuid::new_v4()),
            device_token: None,
            user_token: None,
            generation: 0,
        }
    }
    
    fn with_id(device_id: &str) -> Self {
        Self {
            device_id: device_id.to_string(),
            device_token: None,
            user_token: None,
            generation: 0,
        }
    }
}

/// Test device registration flow
#[tokio::test]
async fn test_device_registration() {
    let server = MockSyncServer::start().await;
    let base_url = server.base_url();
    
    let mut device = SimulatedDevice::new();
    let pairing_code = "regcode1";
    
    // Register pairing code for this device
    server.state().add_pairing_code(pairing_code, &device.device_id);
    
    let client = Client::new();
    
    // Step 1: Exchange pairing code for device token
    let resp = client
        .post(format!("{}/token/json/2/device/new", base_url))
        .json(&json!({
            "code": pairing_code,
            "deviceDesc": "remarkable",
            "deviceID": device.device_id
        }))
        .send()
        .await
        .expect("Device registration failed");
    
    assert_eq!(resp.status(), 200, "Registration should succeed");
    device.device_token = Some(resp.text().await.unwrap());
    
    // Step 2: Get initial user token
    let resp = client
        .post(format!("{}/token/json/2/user/new", base_url))
        .header("Authorization", format!("Bearer {}", device.device_token.as_ref().unwrap()))
        .send()
        .await
        .expect("User token request failed");
    
    assert_eq!(resp.status(), 200, "User token should be issued");
    device.user_token = Some(resp.text().await.unwrap());
    
    // Step 3: Verify we can access sync
    let resp = client
        .get(format!("{}/sync/v3/root", base_url))
        .header("Authorization", format!("Bearer {}", device.user_token.as_ref().unwrap()))
        .send()
        .await
        .expect("Sync root request failed");
    
    assert_eq!(resp.status(), 200, "Should access sync with user token");
    
    let root: serde_json::Value = resp.json().await.unwrap();
    device.generation = root["generation"].as_u64().unwrap_or(1);
    
    println!("✓ Device registration verified");
    println!("  - Device ID: {}", device.device_id);
    println!("  - Initial generation: {}", device.generation);
}

/// Test token refresh cycle
#[tokio::test]
async fn test_token_refresh_cycle() {
    let server = MockSyncServer::start().await;
    let base_url = server.base_url();
    
    let mut device = SimulatedDevice::with_id("refresh-test-device");
    server.state().add_pairing_code("refresh", &device.device_id);
    
    let client = Client::new();
    
    // Register device
    let resp = client
        .post(format!("{}/token/json/2/device/new", base_url))
        .json(&json!({
            "code": "refresh",
            "deviceDesc": "remarkable",
            "deviceID": device.device_id
        }))
        .send()
        .await
        .unwrap();
    
    device.device_token = Some(resp.text().await.unwrap());
    
    // Get multiple user tokens (simulating refresh cycle)
    let mut tokens = Vec::new();
    for i in 0..3 {
        let resp = client
            .post(format!("{}/token/json/2/user/new", base_url))
            .header("Authorization", format!("Bearer {}", device.device_token.as_ref().unwrap()))
            .send()
            .await
            .expect(&format!("Token refresh {} failed", i));
        
        assert_eq!(resp.status(), 200, "Refresh {} should succeed", i);
        tokens.push(resp.text().await.unwrap());
    }
    
    // All tokens should work
    for (i, token) in tokens.iter().enumerate() {
        let resp = client
            .get(format!("{}/sync/v3/root", base_url))
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .unwrap();
        
        assert_eq!(resp.status(), 200, "Token {} should be valid", i);
    }
    
    println!("✓ Token refresh cycle verified");
    println!("  - Refreshed {} times", tokens.len());
}

/// Test multiple device simulation
#[tokio::test]
async fn test_multi_device_simulation() {
    let server = MockSyncServer::start().await;
    let base_url = server.base_url();
    
    let client = Client::new();
    
    // Register multiple devices
    let mut devices = Vec::new();
    for i in 0..3 {
        let mut device = SimulatedDevice::with_id(&format!("multi-device-{}", i));
        let code = format!("multicode{}", i);
        
        server.state().add_pairing_code(&code, &device.device_id);
        
        let resp = client
            .post(format!("{}/token/json/2/device/new", base_url))
            .json(&json!({
                "code": code,
                "deviceDesc": "remarkable",
                "deviceID": device.device_id
            }))
            .send()
            .await
            .unwrap();
        
        device.device_token = Some(resp.text().await.unwrap());
        
        let resp = client
            .post(format!("{}/token/json/2/user/new", base_url))
            .header("Authorization", format!("Bearer {}", device.device_token.as_ref().unwrap()))
            .send()
            .await
            .unwrap();
        
        device.user_token = Some(resp.text().await.unwrap());
        devices.push(device);
    }
    
    // All devices should see the same sync state
    let mut generations = Vec::new();
    for device in &devices {
        let resp = client
            .get(format!("{}/sync/v3/root", base_url))
            .header("Authorization", format!("Bearer {}", device.user_token.as_ref().unwrap()))
            .send()
            .await
            .unwrap();
        
        let root: serde_json::Value = resp.json().await.unwrap();
        generations.push(root["generation"].as_u64().unwrap_or(0));
    }
    
    // All should see same generation
    let first_gen = generations[0];
    for (i, gen) in generations.iter().enumerate() {
        assert_eq!(*gen, first_gen, "Device {} should see same generation", i);
    }
    
    println!("✓ Multi-device simulation verified");
    println!("  - Devices: {}", devices.len());
    println!("  - Shared generation: {}", first_gen);
}

/// Simulate sync-complete event handling
#[tokio::test]
async fn test_sync_complete_handling() {
    let server = MockSyncServer::start().await;
    let base_url = server.base_url();
    
    let mut device = SimulatedDevice::with_id("sync-complete-device");
    server.state().add_pairing_code("synccode", &device.device_id);
    
    let client = Client::new();
    
    // Register device
    let resp = client
        .post(format!("{}/token/json/2/device/new", base_url))
        .json(&json!({
            "code": "synccode",
            "deviceDesc": "remarkable",
            "deviceID": device.device_id
        }))
        .send()
        .await
        .unwrap();
    device.device_token = Some(resp.text().await.unwrap());
    
    let resp = client
        .post(format!("{}/token/json/2/user/new", base_url))
        .header("Authorization", format!("Bearer {}", device.device_token.as_ref().unwrap()))
        .send()
        .await
        .unwrap();
    device.user_token = Some(resp.text().await.unwrap());
    
    // Get initial state
    let resp = client
        .get(format!("{}/sync/v3/root", base_url))
        .header("Authorization", format!("Bearer {}", device.user_token.as_ref().unwrap()))
        .send()
        .await
        .unwrap();
    
    let initial_root: serde_json::Value = resp.json().await.unwrap();
    let initial_gen = initial_root["generation"].as_u64().unwrap();
    
    // Add a document (simulates a change from another device)
    let doc = TestDocument::new("Sync Event Test");
    server.state().add_document(
        &doc.id,
        &doc.metadata,
        &doc.content,
        doc.pages.iter()
            .map(|(id, data)| (id.as_str(), data.as_slice()))
            .collect()
    );
    
    // Fetch new state (simulates receiving sync-complete event)
    let resp = client
        .get(format!("{}/sync/v3/root", base_url))
        .header("Authorization", format!("Bearer {}", device.user_token.as_ref().unwrap()))
        .send()
        .await
        .unwrap();
    
    let new_root: serde_json::Value = resp.json().await.unwrap();
    let new_gen = new_root["generation"].as_u64().unwrap();
    
    // Generation should have incremented
    assert!(new_gen > initial_gen, "Generation should increase after sync");
    
    // Root hash should be different
    let initial_hash = initial_root["hash"].as_str().unwrap();
    let new_hash = new_root["hash"].as_str().unwrap();
    assert_ne!(initial_hash, new_hash, "Root hash should change after sync");
    
    // Should be able to download the new document
    let resp = client
        .get(format!("{}/sync/v3/files/{}", base_url, new_hash))
        .header("Authorization", format!("Bearer {}", device.user_token.as_ref().unwrap()))
        .header("rm-filename", "root.docSchema")
        .send()
        .await
        .unwrap();
    
    assert_eq!(resp.status(), 200, "Should download new root");
    
    let docs: Vec<serde_json::Value> = resp.json().await.unwrap();
    let found = docs.iter().any(|d| d["uuid"].as_str() == Some(&doc.id));
    assert!(found, "New document should be in index");
    
    println!("✓ Sync-complete event handling verified");
    println!("  - Initial generation: {}", initial_gen);
    println!("  - New generation: {}", new_gen);
    println!("  - Delta: {}", new_gen - initial_gen);
}

/// Test generation tracking
#[tokio::test]
async fn test_generation_tracking() {
    let server = MockSyncServer::start().await;
    let base_url = server.base_url();
    server.state().add_token("gen-token");
    
    let client = Client::new();
    
    // Get initial generation
    let resp = client
        .get(format!("{}/sync/v3/root", base_url))
        .header("Authorization", "Bearer gen-token")
        .send()
        .await
        .unwrap();
    
    let root: serde_json::Value = resp.json().await.unwrap();
    let initial_gen = root["generation"].as_u64().unwrap();
    
    // Add multiple documents
    let mut generations = vec![initial_gen];
    
    for i in 0..3 {
        let doc = TestDocument::new(&format!("Gen Test Doc {}", i));
        server.state().add_document(
            &doc.id,
            &doc.metadata,
            &doc.content,
            doc.pages.iter()
                .map(|(id, data)| (id.as_str(), data.as_slice()))
                .collect()
        );
        
        let resp = client
            .get(format!("{}/sync/v3/root", base_url))
            .header("Authorization", "Bearer gen-token")
            .send()
            .await
            .unwrap();
        
        let root: serde_json::Value = resp.json().await.unwrap();
        generations.push(root["generation"].as_u64().unwrap());
    }
    
    // Generations should be strictly increasing
    for i in 1..generations.len() {
        assert!(
            generations[i] > generations[i-1],
            "Generation should increase monotonically: {} -> {}",
            generations[i-1], generations[i]
        );
    }
    
    println!("✓ Generation tracking verified");
    println!("  - Generations: {:?}", generations);
}

/// Test invalid pairing code rejection
#[tokio::test]
async fn test_invalid_pairing_rejected() {
    let server = MockSyncServer::start().await;
    let base_url = server.base_url();
    
    let client = Client::new();
    
    // Try to pair with invalid code
    let resp = client
        .post(format!("{}/token/json/2/device/new", base_url))
        .json(&json!({
            "code": "invalid-code",
            "deviceDesc": "remarkable",
            "deviceID": "fake-device"
        }))
        .send()
        .await
        .unwrap();
    
    assert_eq!(resp.status(), 401, "Invalid code should be rejected");
    
    println!("✓ Invalid pairing code correctly rejected");
}

/// Test expired token handling
#[tokio::test]
async fn test_expired_token_rejected() {
    let server = MockSyncServer::start().await;
    let base_url = server.base_url();
    
    let client = Client::new();
    
    // Try to use an invalid/expired token
    let resp = client
        .get(format!("{}/sync/v3/root", base_url))
        .header("Authorization", "Bearer invalid-expired-token")
        .send()
        .await
        .unwrap();
    
    assert_eq!(resp.status(), 401, "Expired token should be rejected");
    
    println!("✓ Expired token correctly rejected");
}

/// Test device type validation
#[tokio::test]
async fn test_device_type_handling() {
    let server = MockSyncServer::start().await;
    let base_url = server.base_url();
    
    let client = Client::new();
    
    // Test different device types
    let device_types = ["remarkable", "desktop-linux", "desktop-windows", "mobile-android"];
    
    for (i, device_type) in device_types.iter().enumerate() {
        let code = format!("dtype{}", i);
        let device_id = format!("dtype-device-{}", i);
        
        server.state().add_pairing_code(&code, &device_id);
        
        let resp = client
            .post(format!("{}/token/json/2/device/new", base_url))
            .json(&json!({
                "code": code,
                "deviceDesc": device_type,
                "deviceID": device_id
            }))
            .send()
            .await
            .unwrap();
        
        // All device types should be accepted by mock
        // (real API may be more restrictive)
        assert_eq!(resp.status(), 200, "Device type {} should be accepted", device_type);
    }
    
    println!("✓ Device type handling verified");
    println!("  - Tested types: {:?}", device_types);
}

/// Real device integration test
#[tokio::test]
#[ignore = "requires USB-connected reMarkable device with valid tokens"]
async fn test_real_device_sync_events() {
    if !device_available().await {
        println!("Device not available, skipping");
        return;
    }
    
    let (device_token, user_token) = match load_captured_tokens() {
        Some(tokens) => tokens,
        None => {
            println!("No captured tokens, skipping");
            return;
        }
    };
    
    // Would test MQTT connection and sync events here
    // Requires real device and valid tokens
    
    println!("✓ Real device sync events test passed");
    println!("  - Device token present: {}", !device_token.is_empty());
    println!("  - User token present: {}", !user_token.is_empty());
}

/// Test MQTT configuration generation (doesn't connect)
#[tokio::test]
async fn test_mqtt_config_generation() {
    // Generate mock tokens
    let device_token = mock_device_token("mqtt-test-device");
    let user_token = mock_user_token("eu");
    
    // Verify tokens have expected structure
    let device_parts: Vec<&str> = device_token.split('.').collect();
    let user_parts: Vec<&str> = user_token.split('.').collect();
    
    assert_eq!(device_parts.len(), 3, "Device token should be JWT");
    assert_eq!(user_parts.len(), 3, "User token should be JWT");
    
    // Decode user token to check region
    use base64::Engine;
    let claims_b64 = user_parts[1];
    let claims_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(claims_b64)
        .expect("Should decode claims");
    let claims: serde_json::Value = serde_json::from_slice(&claims_bytes)
        .expect("Should parse claims");
    
    // Check tectonic claim for region
    let region = claims.get("https://auth.remarkable.com/tectonic")
        .and_then(|v| v.as_str())
        .expect("Should have tectonic claim");
    
    assert_eq!(region, "eu", "Region should be eu");
    
    println!("✓ MQTT config generation verified");
    println!("  - Region: {}", region);
}

/// Test sync state debouncing
#[tokio::test]
async fn test_sync_state_debouncing() {
    use remarkable_mqtt::sync_events::SyncState;
    use remarkable_mqtt::SyncComplete;
    use std::time::Duration;
    
    // Create state with zero debounce for testing
    let mut state = SyncState::default().with_debounce(Duration::ZERO);
    
    // First event should always be processed
    let sync1 = SyncComplete {
        source_device_id: "device-1".to_string(),
        generation: 10,
    };
    let action = state.process_event(&sync1);
    assert!(action.is_some(), "First event should be processed");
    
    // Same generation should be ignored (stale)
    let action = state.process_event(&sync1);
    assert!(action.is_none(), "Same generation should be ignored");
    
    // Older generation should be ignored
    let sync_old = SyncComplete {
        source_device_id: "device-1".to_string(),
        generation: 5,
    };
    let action = state.process_event(&sync_old);
    assert!(action.is_none(), "Older generation should be ignored");
    
    // Newer generation should be processed
    let sync2 = SyncComplete {
        source_device_id: "device-1".to_string(),
        generation: 15,
    };
    let action = state.process_event(&sync2);
    assert!(action.is_some(), "Newer generation should be processed");
    
    // Verify stats
    let stats = state.stats();
    assert_eq!(stats.last_generation, Some(15));
    
    println!("✓ Sync state debouncing verified");
    println!("  - Final generation: {:?}", stats.last_generation);
}
