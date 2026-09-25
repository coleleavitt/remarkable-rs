//! Screen share signaling messages
//!
//! The WebRTC session for screen share is negotiated through the MQTT broker
//! (reMarkable's cloud, or a self-hosted one such as remarkable-server). Verified
//! against a tablet and matching xochitl's `screenshare::Broker`:
//!
//! 1. Connect with `client_id = username = <cid>` and the user token as password,
//!    then subscribe to [`subscription`] (`user/{uid}/#`).
//! 2. Publish every request to [`signaling_topic`]:
//!    - [`SignalingRequest::JoinActiveRoom`] → [`SignalingEvent::RoomJoined`] or
//!      [`SignalingEvent::RoomNotFound`] (screen share is off on the tablet)
//!    - [`SignalingRequest::Broadcast`] of [`PeerMessage::RequestOffer`]
//! 3. The tablet answers with [`SignalingEvent::Direct`] messages carrying
//!    [`PeerMessage::WebRtc`]: an offer, then trickled ICE candidates.
//! 4. Reply with [`SignalingRequest::Direct`] to the tablet's client id: the
//!    answer and our own candidates.
//!
//! The screen itself then flows peer-to-peer over the `screenshare` data
//! channel; see the `remarkable-screenshare` crate.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::MqttError;

/// Topic a client publishes its signaling requests to.
pub fn signaling_topic(user_id: &str, client_id: &str) -> String {
    format!("remarkable/screenshare/signaling/user/{user_id}/client/{client_id}/signaling")
}

/// Subscription that receives every broker reply for `user_id`.
///
/// Replies arrive on `user/{uid}/signaling`, `user/{uid}/client/{cid}/signaling/{room}`
/// and `user/{uid}/client/{cid}/signaling/room/{room}`; the message body says
/// what it is, so clients subscribe to the whole user tree.
pub fn subscription(user_id: &str) -> String {
    format!("user/{user_id}/#")
}

/// Request a client publishes to [`signaling_topic`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum SignalingRequest {
    /// Create a room (the tablet does this when screen share is turned on).
    CreateRoom {
        #[serde(default)]
        room: String,
    },
    /// Join the user's active screen share room.
    JoinActiveRoom {
        #[serde(default)]
        room: String,
        #[serde(rename = "roomId", default)]
        room_id: String,
    },
    /// Relay `payload` to every other participant of the room.
    Broadcast {
        #[serde(rename = "roomId")]
        room_id: String,
        payload: PeerMessage,
    },
    /// Relay `payload` to one participant.
    Direct {
        #[serde(rename = "roomId")]
        room_id: String,
        #[serde(rename = "clientId")]
        client_id: String,
        payload: PeerMessage,
    },
}

/// Message the broker delivers to a client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum SignalingEvent {
    RoomCreated {
        #[serde(default)]
        room: String,
        #[serde(rename = "roomId")]
        room_id: String,
    },
    RoomJoined {
        #[serde(rename = "roomId")]
        room_id: String,
        /// ICE server configuration, e.g. `{"ice_servers": [{"url": ...}]}`.
        #[serde(rename = "iceServers", default, skip_serializing_if = "Option::is_none")]
        ice_servers: Option<Value>,
    },
    /// No active room: screen share is not on.
    RoomNotFound,
    /// A participant's broadcast, relayed.
    Broadcast {
        #[serde(rename = "clientId")]
        client_id: String,
        payload: PeerMessage,
    },
    /// A participant's direct message, relayed.
    Direct {
        #[serde(rename = "clientId")]
        client_id: String,
        payload: PeerMessage,
    },
}

/// Payload exchanged between room participants.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PeerMessage {
    /// Ask the tablet for a WebRTC offer; `id` is the requester's client id.
    #[serde(rename = "request-offer")]
    RequestOffer { id: String },
    /// WebRTC negotiation. xochitl spells the tag `webtrc`.
    #[serde(rename = "webtrc")]
    WebRtc { payload: WebRtcMessage },
}

/// SDP and ICE messages carried by [`PeerMessage::WebRtc`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum WebRtcMessage {
    Offer { description: String },
    Answer { description: String },
    Candidate {
        candidate: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mid: Option<String>,
    },
}

impl SignalingRequest {
    /// Serialize for publishing.
    pub fn to_bytes(&self) -> Result<Vec<u8>, MqttError> {
        serde_json::to_vec(self).map_err(|e| MqttError::Publish(e.to_string()))
    }
}

impl SignalingEvent {
    /// Parse a broker message; `None` for anything that is not screen share signaling.
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        serde_json::from_slice(data).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn topics() {
        assert_eq!(
            signaling_topic("local-user", "viewer-1"),
            "remarkable/screenshare/signaling/user/local-user/client/viewer-1/signaling"
        );
        assert_eq!(subscription("local-user"), "user/local-user/#");
    }

    #[test]
    fn requests_match_the_wire_format() {
        let join = SignalingRequest::JoinActiveRoom { room: String::new(), room_id: String::new() };
        assert_eq!(
            serde_json::to_value(&join).unwrap(),
            json!({"type": "join-active-room", "room": "", "roomId": ""})
        );

        let request_offer = SignalingRequest::Broadcast {
            room_id: "R".into(),
            payload: PeerMessage::RequestOffer { id: "viewer-1".into() },
        };
        assert_eq!(
            serde_json::to_value(&request_offer).unwrap(),
            json!({"type": "broadcast", "roomId": "R", "payload": {"type": "request-offer", "id": "viewer-1"}})
        );

        let answer = SignalingRequest::Direct {
            room_id: "R".into(),
            client_id: "T".into(),
            payload: PeerMessage::WebRtc { payload: WebRtcMessage::Answer { description: "v=0".into() } },
        };
        assert_eq!(
            serde_json::to_value(&answer).unwrap(),
            json!({"type": "direct", "roomId": "R", "clientId": "T",
                   "payload": {"type": "webtrc", "payload": {"type": "answer", "description": "v=0"}}})
        );
    }

    #[test]
    fn events_parse_from_the_wire_format() {
        let joined = br#"{"type":"room-joined","roomId":"R","iceServers":{"ice_servers":[]}}"#;
        assert_eq!(
            SignalingEvent::from_bytes(joined),
            Some(SignalingEvent::RoomJoined { room_id: "R".into(), ice_servers: Some(json!({"ice_servers": []})) })
        );
        assert_eq!(SignalingEvent::from_bytes(br#"{"type":"room-not-found"}"#), Some(SignalingEvent::RoomNotFound));

        let offer = br#"{"type":"direct","clientId":"T","payload":{"type":"webtrc","payload":{"type":"offer","description":"v=0"}}}"#;
        assert_eq!(
            SignalingEvent::from_bytes(offer),
            Some(SignalingEvent::Direct {
                client_id: "T".into(),
                payload: PeerMessage::WebRtc { payload: WebRtcMessage::Offer { description: "v=0".into() } },
            })
        );

        let candidate = br#"{"type":"direct","clientId":"T","payload":{"type":"webtrc","payload":{"type":"candidate","candidate":"candidate:1 1 UDP 1 10.0.0.2 5000 typ host","mid":"0"}}}"#;
        assert!(matches!(
            SignalingEvent::from_bytes(candidate),
            Some(SignalingEvent::Direct { payload: PeerMessage::WebRtc { payload: WebRtcMessage::Candidate { mid: Some(m), .. } }, .. }) if m == "0"
        ));
    }

    #[test]
    fn unrelated_messages_are_ignored() {
        assert_eq!(SignalingEvent::from_bytes(br#"{"type":"sync-complete"}"#), None);
        assert_eq!(SignalingEvent::from_bytes(b"not json"), None);
    }
}
