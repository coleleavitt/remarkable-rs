#![forbid(unsafe_code)]
//! reMarkable screen share: receive the tablet's screen over WebRTC.
//!
//! The tablet shares its screen as an RFB-like byte stream ([`rfb`]) on a
//! WebRTC data channel ([`webrtc`]). [`session::pump_frames`] turns that
//! channel into grayscale [`Frame`]s. Negotiating the connection needs a
//! signaling broker; message types live in `remarkable_mqtt::screenshare`.
//!
//! # Features
//!
//! - default: the protocol, transport and frame session only. Embed these
//!   when you already have a signaling path (e.g. inside remarkable-server).
//! - `cloud`: [`cloud::connect`], a client that negotiates through a
//!   remarkable-server MQTT broker.
//! - `app`: the `screenshare` binary and its pieces: a browser viewer
//!   (`server`), USB framebuffer capture (`usb`) and PNG/JPEG recording.
//!
//! # Example
//!
//! ```ignore
//! let (webrtc, ice_rx, mut data_rx) = WebRtcHandler::new(TransportConfig::default()).await?;
//! let answer = webrtc.accept_offer(&tablet_offer).await?;
//! // ... signal `answer` and the candidates from `ice_rx` to the tablet ...
//! pump_frames(&mut data_rx, |frame| show(frame)).await?;
//! ```

pub mod error;
pub mod rfb;
pub mod session;
pub mod webrtc;

#[cfg(feature = "cloud")]
pub mod cloud;

#[cfg(feature = "app")]
pub mod recorder;
#[cfg(feature = "app")]
pub mod server;
#[cfg(feature = "app")]
pub mod usb;
#[cfg(feature = "app")]
pub mod viewer;

pub use error::{Error, Result};
pub use rfb::RfbDecoder;
pub use session::{pump_frames, Frame};
pub use webrtc::{TransportConfig, WebRtcHandler};
