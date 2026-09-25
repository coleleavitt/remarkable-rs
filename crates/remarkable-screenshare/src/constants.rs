//! Protocol constants for reMarkable screen share
//!
//! Based on reverse engineering of xochitl firmware and iOS app.

/// MQTT broker production endpoint
pub const MQTT_BROKER_PROD: &str = "vernemq-prod.cloud.remarkable.engineering";
/// MQTT WebSocket broker endpoint
pub const MQTT_BROKER_WS_PROD: &str = "vernemq-ws-prod.cloud.remarkable.engineering";
/// MQTT TLS port
pub const MQTT_PORT_TLS: u16 = 8883;
/// MQTT WebSocket port
pub const MQTT_PORT_WS: u16 = 443;
/// MQTT WebSocket path
pub const MQTT_WS_PATH: &str = "/mqtt";

/// Regional broker template
pub const MQTT_BROKER_TEMPLATE: &str = "vernemq-{env}.cloud.remarkable.engineering";

/// Signaling topic for user
pub const TOPIC_SIGNALING_USER: &str = "remarkable/screenshare/signaling/user/{}";
/// Signaling topic for client
pub const TOPIC_SIGNALING_CLIENT: &str = "remarkable/screenshare/signaling/user/{}/client/{}";
/// Signaling topic for room
pub const TOPIC_SIGNALING_ROOM: &str = "remarkable/screenshare/signaling/user/{}/client/{}/room/{}";
/// Topic subscription pattern
pub const TOPIC_SUBSCRIBE_PATTERN: &str = "user/{}/signaling/#";

/// WebRTC signaling message types
pub mod msg_type {
    pub const REQUEST_OFFER: &str = "request-offer";
    pub const OFFER: &str = "offer";
    pub const ANSWER: &str = "answer";
    pub const CANDIDATE: &str = "candidate";
    pub const ROOM_CREATED: &str = "room-created";
    pub const CONNECTION_DECLINED: &str = "connectionDeclined";
    pub const SCREENSHARE_DECLINED: &str = "screenShareDeclined";
}

/// RFB protocol version
pub const RFB_VERSION: &str = "RFB 003.008";

/// reMarkable 2 framebuffer dimensions
pub const FB_WIDTH: u32 = 1872;
pub const FB_HEIGHT: u32 = 1404;
/// Bits per pixel (8-bit grayscale)
pub const FB_BPP: u8 = 8;

/// reMarkable Paper Pro framebuffer dimensions
pub const FB_WIDTH_PAPER_PRO: u32 = 2160;
pub const FB_HEIGHT_PAPER_PRO: u32 = 2880;

/// USB IP address
pub const USB_IP: &str = "10.11.99.1";
pub const USB_SSH_PORT: u16 = 22;
pub const USB_USER: &str = "root";

/// Framebuffer device path on device
pub const FB_DEVICE_PATH: &str = "/dev/fb0";

/// RFB message types - Client to Server
pub mod rfb_client_msg {
    pub const SET_PIXEL_FORMAT: u8 = 0;
    pub const SET_ENCODINGS: u8 = 2;
    pub const FB_UPDATE_REQUEST: u8 = 3;
    pub const KEY_EVENT: u8 = 4;
    pub const POINTER_EVENT: u8 = 5;
    pub const CLIENT_CUT_TEXT: u8 = 6;
}

/// RFB message types - Server to Client
pub mod rfb_server_msg {
    pub const FB_UPDATE: u8 = 0;
    pub const SET_COLOR_MAP: u8 = 1;
    pub const BELL: u8 = 2;
    pub const SERVER_CUT_TEXT: u8 = 3;
}

/// RFB encoding types
pub mod rfb_encoding {
    pub const RAW: i32 = 0;
    pub const COPYRECT: i32 = 1;
    pub const RRE: i32 = 2;
    pub const HEXTILE: i32 = 5;
    pub const TRLE: i32 = 15;
    pub const ZRLE: i32 = 16;
    pub const TIGHT: i32 = 7;
    pub const ZLIBHEX: i32 = 8;
}

/// Default STUN servers for WebRTC
pub const DEFAULT_STUN_SERVERS: &[&str] = &[
    "stun:stun.l.google.com:19302",
    "stun:stun1.l.google.com:19302",
];

/// Web server default port
pub const WEB_SERVER_PORT: u16 = 8088;
