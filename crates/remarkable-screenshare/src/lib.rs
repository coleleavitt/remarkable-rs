#![forbid(unsafe_code)]
//! reMarkable Screen Share Viewer
//!
//! A comprehensive screen share solution for reMarkable tablets supporting:
//! - WebRTC-based browser streaming (modern protocol)
//! - USB framebuffer direct capture (offline, low-latency)
//! - MQTT signaling integration
//! - Recording capability (WebM/MP4)
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                    remarkable-screenshare                        │
//! ├─────────────────────────────────────────────────────────────────┤
//! │  ┌─────────────┐   ┌─────────────┐   ┌─────────────┐           │
//! │  │  WebRTC     │   │   MQTT      │   │    USB      │           │
//! │  │  Viewer     │   │  Signaling  │   │  Capture    │           │
//! │  └──────┬──────┘   └──────┬──────┘   └──────┬──────┘           │
//! │         │                 │                 │                   │
//! │  ┌──────┴─────────────────┴─────────────────┴──────┐           │
//! │  │              Frame Processor                     │           │
//! │  │  (RFB decode, grayscale conversion, resize)      │           │
//! │  └──────────────────────┬───────────────────────────┘           │
//! │                         │                                       │
//! │  ┌──────────────────────┴───────────────────────────┐           │
//! │  │              Output Layer                         │           │
//! │  │  - Browser (WebSocket → HTML5 Canvas)             │           │
//! │  │  - Recording (GStreamer → WebM/MP4)               │           │
//! │  │  - Frame export (PNG sequence)                    │           │
//! │  └───────────────────────────────────────────────────┘           │
//! └─────────────────────────────────────────────────────────────────┘
//! ```

pub mod constants;
pub mod error;
pub mod mqtt;
pub mod rfb;
pub mod usb;
pub mod webrtc;
pub mod cloud;
pub mod viewer;
pub mod recorder;
pub mod server;
pub mod token;

pub use error::{Error, Result};
pub use constants::*;

/// Re-export common types
pub mod prelude {
    pub use crate::constants::*;
    pub use crate::error::{Error, Result};
    pub use crate::mqtt::MqttSignaling;
    pub use crate::rfb::RfbDecoder;
    pub use crate::usb::UsbCapture;
    pub use crate::viewer::ScreenShareViewer;
    pub use crate::server::WebServer;
}
