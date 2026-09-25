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
use tokio::sync::{broadcast, watch};
use tower_http::cors::CorsLayer;
use tracing::{debug, info, warn};

use crate::error::{Error, Result};
use crate::session::Frame;
use crate::viewer::{ScreenShareViewer, ViewerSource, ViewerState};

/// Default port of the browser viewer.
pub const WEB_SERVER_PORT: u16 = 8088;

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
    /// Latest frame as `(sequence, PNG)`, so late joiners get a picture straight
    /// away instead of waiting for the tablet screen to change. The sequence
    /// pairs each frame with the cursor reported against it.
    latest_png: watch::Receiver<Option<(u64, Arc<Vec<u8>>)>>,
    /// Pen position tagged with the frame sequence it belongs to.
    cursor: watch::Receiver<(u64, Option<(u32, u32)>)>,
}

/// Web server
pub struct WebServer {
    config: ServerConfig,
    source: ViewerSource,
}

impl WebServer {
    /// Create new web server
    pub fn new(config: ServerConfig, source: ViewerSource) -> Self {
        Self { config, source }
    }
    
    /// Run the server
    pub async fn run(&self) -> Result<()> {
        let viewer = Arc::new(ScreenShareViewer::new(self.source.clone()));
        let (png_tx, latest_png) = watch::channel(None);
        // Each frame carries a sequence, and each cursor is tagged with the
        // sequence of the frame it belongs to, so the browser can pair them
        // exactly instead of relying on delivery timing over two channels.
        let (cursor_tx, cursor_rx) = watch::channel((0u64, None));

        let state = Arc::new(AppState {
            viewer: viewer.clone(),
            latest_png,
            cursor: cursor_rx,
        });

        // Subscribe before starting so the first frame is not missed.
        let mut frames = viewer.subscribe();
        let mut cursor = viewer.cursor();
        viewer.start().await?;

        tokio::spawn(async move {
            // Frames encoded so far; also the sequence a cursor is tagged with.
            let mut seq: u64 = 0;
            loop {
                tokio::select! {
                    frame = frames.recv() => match frame {
                        Ok(frame) => match frame_to_png(&frame) {
                            Ok(png) => {
                                seq += 1;
                                png_tx.send_replace(Some((seq, Arc::new(png))));
                            }
                            Err(e) => warn!("Failed to encode frame: {}", e),
                        },
                        // Only the newest frame matters; skip what we missed.
                        Err(broadcast::error::RecvError::Lagged(n)) => debug!("Skipped {} frames", n),
                        Err(broadcast::error::RecvError::Closed) => break,
                    },
                    changed = cursor.changed() => {
                        if changed.is_err() { break }
                        // Tag the cursor with the frame it was reported against.
                        cursor_tx.send_replace((seq, *cursor.borrow_and_update()));
                    }
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
    let mut cursor = state.cursor.clone();
    
    info!("WebSocket client connected");
    
    // Send the current frame, then each newer one as a binary message of an
    // 8-byte big-endian sequence followed by the PNG, and pen moves as
    // `{"cursor":[x,y],"seq":n}` / `{"cursor":null,"seq":n}` text. The browser
    // pairs a cursor with its frame by sequence, so send order doesn't matter.
    let send_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                changed = latest.changed() => {
                    if changed.is_err() { break }
                    // Bind the clone so the watch guard is dropped before the await.
                    let frame = latest.borrow_and_update().clone();
                    if let Some((seq, png)) = frame {
                        let mut msg = seq.to_be_bytes().to_vec();
                        msg.extend_from_slice(&png);
                        if sender.send(Message::Binary(msg)).await.is_err() { break }
                    }
                }
                changed = cursor.changed() => {
                    if changed.is_err() { break }
                    let (seq, point) = *cursor.borrow_and_update();
                    let msg = Message::Text(
                        serde_json::json!({ "cursor": point.map(|(x, y)| [x, y]), "seq": seq }).to_string(),
                    );
                    if sender.send(msg).await.is_err() { break }
                }
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
        Ok(Ok(frame)) => frame.clone().map(|(_seq, png)| png),
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
    let mut buffer = std::io::Cursor::new(Vec::new());
    frame
        .to_image()?
        .write_to(&mut buffer, image::ImageFormat::Png)
        .map_err(|e| Error::Framebuffer(format!("PNG encode error: {}", e)))?;
    Ok(buffer.into_inner())
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
        // Last frame image and pen position; the pen is a 15 px dot, as in the
        // desktop app. Each frame and cursor carries a sequence so the cursor is
        // only drawn once the frame it belongs to has been shown (or resolved),
        // never landing on an unrelated picture.
        let frame = null, cursorPoint = null, cursorSeq = 0;
        let displayedSeq = 0, resolvedSeq = 0;
        function render() {
            if (!frame) return;
            ctx.drawImage(frame, 0, 0);
            if (cursorPoint && cursorSeq <= resolvedSeq) {
                ctx.fillStyle = 'rgba(244, 21, 21, 0.8)'; // desktop QML hoverCursorColor #CCF41515
                ctx.beginPath();
                ctx.arc(cursorPoint[0], cursorPoint[1], 7.5, 0, 2 * Math.PI);
                ctx.fill();
            }
        }
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
                if (typeof event.data === 'string') {
                    const m = JSON.parse(event.data);
                    cursorPoint = m.cursor;
                    cursorSeq = m.seq || 0;
                    // Drawn now if its frame is already resolved, else when it is.
                    render();
                    return;
                }
                // Binary: 8-byte big-endian frame sequence, then the PNG bytes.
                const view = new DataView(event.data);
                const seq = Number(view.getBigUint64(0, false));
                const blob = new Blob([new Uint8Array(event.data, 8)], { type: 'image/png' });
                const img = new Image();
                const url = URL.createObjectURL(blob);
                img.onload = () => {
                    URL.revokeObjectURL(url);
                    if (seq > resolvedSeq) resolvedSeq = seq;
                    // Never regress to a frame older than the one on screen.
                    if (seq <= displayedSeq) { render(); return; }
                    if (canvas.width !== img.width || canvas.height !== img.height) {
                        canvas.width = img.width;
                        canvas.height = img.height;
                    }
                    frame = img;
                    displayedSeq = seq;
                    render();
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
                img.onerror = () => {
                    URL.revokeObjectURL(url);
                    // A failed frame is still "resolved" so cursors waiting on it
                    // (or a later frame) aren't stranded; it never becomes current.
                    if (seq > resolvedSeq) resolvedSeq = seq;
                    render();
                };
                img.src = url;

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
