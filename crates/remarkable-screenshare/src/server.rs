//! Web Server for Browser-Based Viewing
//!
//! Provides a web interface with WebSocket streaming for browser-based viewing.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};
use futures::{sink::SinkExt, stream::StreamExt};
use image::codecs::png::PngEncoder;
use image::{GrayImage, ImageBuffer, ImageEncoder};
use tokio::sync::{broadcast, watch};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tracing::{debug, error, info, warn};

use crate::constants::WEB_SERVER_PORT;
use crate::error::{Error, Result};
use crate::usb::Frame;
use crate::viewer::{ScreenShareViewer, ViewerConfig, ViewerState};

/// Server configuration
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub port: u16,
    pub host: String,
    pub static_dir: Option<PathBuf>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: WEB_SERVER_PORT,
            host: "0.0.0.0".to_string(),
            static_dir: None,
        }
    }
}

/// Shared server state
struct AppState {
    viewer: Arc<ScreenShareViewer>,
    /// Latest frame as PNG, so late joiners get a picture straight away
    /// instead of waiting for the tablet screen to change.
    latest_png: watch::Receiver<Option<Arc<Vec<u8>>>>,
}

/// Web server
pub struct WebServer {
    config: ServerConfig,
    viewer_config: ViewerConfig,
}

impl WebServer {
    /// Create new web server
    pub fn new(config: ServerConfig, viewer_config: ViewerConfig) -> Self {
        Self { config, viewer_config }
    }
    
    /// Run the server
    pub async fn run(&self) -> Result<()> {
        let viewer = Arc::new(ScreenShareViewer::new(self.viewer_config.clone()));
        let (png_tx, latest_png) = watch::channel(None);
        
        let state = Arc::new(AppState {
            viewer: viewer.clone(),
            latest_png,
        });
        
        // Subscribe before starting so the first frame is not missed.
        let mut rx = viewer.subscribe();
        viewer.start().await?;
        
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(frame) => match frame_to_png(&frame) {
                        Ok(png) => {
                            png_tx.send_replace(Some(Arc::new(png)));
                        }
                        Err(e) => warn!("Failed to encode frame: {}", e),
                    },
                    // Only the newest frame matters; skip what we missed.
                    Err(broadcast::error::RecvError::Lagged(n)) => debug!("Skipped {} frames", n),
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
        
        // Build router
        let app = Router::new()
            .route("/", get(index_handler))
            .route("/ws", get(ws_handler))
            .route("/api/status", get(status_handler))
            .route("/api/frame", get(frame_handler))
            .layer(CorsLayer::permissive())
            .with_state(state);
        
        let addr: SocketAddr = format!("{}:{}", self.config.host, self.config.port)
            .parse()
            .map_err(|e| Error::Server(format!("Invalid address: {}", e)))?;
        
        info!("Starting web server at http://{}", addr);
        
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| Error::Server(format!("Failed to bind: {}", e)))?;
        
        axum::serve(listener, app)
            .await
            .map_err(|e| Error::Server(format!("Server error: {}", e)))?;
        
        Ok(())
    }
}

/// Index page handler
async fn index_handler() -> Html<&'static str> {
    Html(INDEX_HTML)
}

/// WebSocket handler
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

/// Handle WebSocket connection
async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();
    let mut latest = state.latest_png.clone();
    latest.mark_changed();
    
    info!("WebSocket client connected");
    
    // Send the current frame, then each newer one.
    let send_task = tokio::spawn(async move {
        while latest.changed().await.is_ok() {
            let Some(png) = latest.borrow_and_update().clone() else { continue };
            if sender.send(Message::Binary(png.to_vec())).await.is_err() {
                break;
            }
        }
    });
    
    // Handle incoming messages (for control commands)
    while let Some(Ok(msg)) = receiver.next().await {
        match msg {
            Message::Text(text) => {
                debug!("Received text: {}", text);
            }
            Message::Close(_) => {
                break;
            }
            _ => {}
        }
    }
    
    send_task.abort();
    info!("WebSocket client disconnected");
}

/// Status handler
async fn status_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let viewer_state = state.viewer.state().await;
    let status = match viewer_state {
        ViewerState::Disconnected => "disconnected",
        ViewerState::Connecting => "connecting",
        ViewerState::Connected => "connected",
        ViewerState::Streaming => "streaming",
        ViewerState::Error => "error",
    };
    
    serde_json::json!({
        "status": status
    }).to_string()
}

/// Single frame handler
async fn frame_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let mut latest = state.latest_png.clone();
    let png = match tokio::time::timeout(std::time::Duration::from_secs(10), latest.wait_for(Option::is_some)).await {
        Ok(Ok(png)) => png.clone(),
        _ => None,
    };
    match png {
        Some(png) => Response::builder()
            .header("Content-Type", "image/png")
            .body(axum::body::Body::from(png.to_vec()))
            .unwrap(),
        None => Response::builder()
            .status(503)
            .body(axum::body::Body::from("No frame received yet"))
            .unwrap(),
    }
}

/// Convert frame to PNG bytes
fn frame_to_png(frame: &Frame) -> Result<Vec<u8>> {
    let img: GrayImage = ImageBuffer::from_raw(frame.width, frame.height, frame.data.clone())
        .ok_or_else(|| Error::Framebuffer("Invalid frame dimensions".into()))?;
    
    let mut buffer = Vec::new();
    let encoder = PngEncoder::new(&mut buffer);
    encoder
        .write_image(&img, frame.width, frame.height, image::ExtendedColorType::L8)
        .map_err(|e| Error::Framebuffer(format!("PNG encode error: {}", e)))?;
    
    Ok(buffer)
}

/// Embedded HTML page
const INDEX_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>reMarkable Screen Share</title>
    <style>
        * {
            margin: 0;
            padding: 0;
            box-sizing: border-box;
        }
        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            background: #1a1a1a;
            color: #fff;
            min-height: 100vh;
            display: flex;
            flex-direction: column;
            align-items: center;
            padding: 20px;
        }
        h1 {
            margin-bottom: 20px;
            font-weight: 300;
        }
        .status {
            margin-bottom: 20px;
            padding: 10px 20px;
            border-radius: 20px;
            font-size: 14px;
        }
        .status.connected { background: #2d5a2d; }
        .status.connecting { background: #5a5a2d; }
        .status.disconnected { background: #5a2d2d; }
        .status.streaming { background: #2d5a5a; }
        .canvas-container {
            background: #fff;
            border-radius: 10px;
            padding: 10px;
            box-shadow: 0 10px 40px rgba(0,0,0,0.3);
        }
        canvas {
            display: block;
            max-width: 100%;
            height: auto;
        }
        .controls {
            margin-top: 20px;
            display: flex;
            gap: 10px;
        }
        button {
            padding: 10px 20px;
            border: none;
            border-radius: 5px;
            cursor: pointer;
            font-size: 14px;
            transition: opacity 0.2s;
        }
        button:hover { opacity: 0.8; }
        .btn-primary { background: #4a9eff; color: #fff; }
        .btn-secondary { background: #444; color: #fff; }
        .stats {
            margin-top: 20px;
            font-size: 12px;
            color: #888;
        }
    </style>
</head>
<body>
    <h1>reMarkable Screen Share</h1>
    <div id="status" class="status disconnected">Disconnected</div>
    <div class="canvas-container">
        <canvas id="screen" width="1872" height="1404"></canvas>
    </div>
    <div class="controls">
        <button class="btn-primary" onclick="connect()">Connect</button>
        <button class="btn-secondary" onclick="disconnect()">Disconnect</button>
        <button class="btn-secondary" onclick="saveFrame()">Save Frame</button>
    </div>
    <div class="stats" id="stats">FPS: 0 | Frames: 0</div>

    <script>
        const canvas = document.getElementById('screen');
        const ctx = canvas.getContext('2d');
        const statusEl = document.getElementById('status');
        const statsEl = document.getElementById('stats');
        
        let ws = null;
        let frameCount = 0;
        let lastFpsTime = Date.now();
        let fpsCount = 0;
        let currentFps = 0;

        function connect() {
            if (ws) ws.close();
            
            const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
            ws = new WebSocket(`${protocol}//${location.host}/ws`);
            ws.binaryType = 'arraybuffer';
            
            ws.onopen = () => {
                statusEl.textContent = 'Connected';
                statusEl.className = 'status connected';
            };
            
            ws.onclose = () => {
                statusEl.textContent = 'Disconnected';
                statusEl.className = 'status disconnected';
            };
            
            ws.onmessage = async (event) => {
                const blob = new Blob([event.data], { type: 'image/png' });
                const img = new Image();
                img.onload = () => {
                    if (canvas.width !== img.width || canvas.height !== img.height) {
                        canvas.width = img.width;
                        canvas.height = img.height;
                    }
                    ctx.drawImage(img, 0, 0);
                    frameCount++;
                    fpsCount++;
                    
                    const now = Date.now();
                    if (now - lastFpsTime >= 1000) {
                        currentFps = fpsCount;
                        fpsCount = 0;
                        lastFpsTime = now;
                    }
                    statsEl.textContent = `FPS: ${currentFps} | Frames: ${frameCount}`;
                };
                img.src = URL.createObjectURL(blob);
                
                statusEl.textContent = 'Streaming';
                statusEl.className = 'status streaming';
            };
        }
        
        function disconnect() {
            if (ws) {
                ws.close();
                ws = null;
            }
        }
        
        function saveFrame() {
            const link = document.createElement('a');
            link.download = `remarkable-${Date.now()}.png`;
            link.href = canvas.toDataURL('image/png');
            link.click();
        }
        
        // Auto-connect
        connect();
    </script>
</body>
</html>"#;
