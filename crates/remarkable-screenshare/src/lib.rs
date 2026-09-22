//! reMarkable Screen Share Library
//!
//! A Rust implementation of the reMarkable screen share protocol.
//!
//! # Protocol Overview
//!
//! 1. **MQTT Signaling**: Connect to VerneMQ broker with device tokens
//! 2. **WebRTC Negotiation**: Exchange SDP offers/answers and ICE candidates
//! 3. **RFB Data**: Receive framebuffer updates over WebRTC DataChannel
//!
//! # Features
//!
//! - **Cloud Screen Share**: MQTT + WebRTC + RFB protocol
//! - **USB Capture**: Direct framebuffer capture via SSH
//! - **Display**: Real-time window display with minifb
//! - **Export**: PNG snapshots and GIF recording
//! - **Input Injection**: Send keyboard and mouse events (optional)
//!
//! # Example
//!
//! ```ignore
//! use remarkable_screenshare::{ClientConfig, ScreenShareClient};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = ClientConfig::from_files(
//!         "device_token.txt",
//!         "user_token.txt",
//!     )?
//!     .with_display(true)
//!     .with_output("./frames");
//!
//!     let mut client = ScreenShareClient::new(config);
//!     client.run().await?;
//!
//!     Ok(())
//! }
//! ```

pub mod client;
pub mod display;
pub mod export;
pub mod rfb;
pub mod signaling;
pub mod usb;
pub mod webrtc;

// Re-exports
pub use client::{ClientConfig, ClientError, ClientState, ScreenShareClient};
pub use display::{Display, DisplayError, InputEvent};
pub use export::{ExportError, GifRecorder, PngExporter};
pub use rfb::{
    Encoding, FramebufferUpdate, PixelFormat, Rectangle, RfbDecoder, RfbEncoder, RfbError,
    FB_BPP, FB_HEIGHT, FB_WIDTH,
};
pub use signaling::{
    MqttSignaling, SignalingConfig, SignalingError, SignalingEvent, SignalingMessage,
    SignalingState,
};
pub use usb::{FramebufferInfo, UsbCapture, UsbConfig, UsbError};
pub use webrtc::{ConnectionState, IceCandidate, WebRtcError, WebRtcEvent, WebRtcHandler};
