//! Screen share signaling support
//!
//! Implements WebRTC signaling for reMarkable screen share feature.
//!
//! # Protocol Overview
//!
//! Screen share uses MQTT for WebRTC signaling:
//! 1. Device publishes offer to `user/{id}/client/{id}/screenshare`
//! 2. Viewer sends answer back
//! 3. ICE candidates exchanged for NAT traversal
//!
//! # Message Types
//!
//! - `offer` - WebRTC SDP offer from device
//! - `answer` - WebRTC SDP answer from viewer
//! - `ice-candidate` - ICE candidate for NAT traversal
//! - `start` - Request to start screen share
//! - `stop` - Request to stop screen share

use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::{MqttClient, MqttError, Topic};

/// Screen share signaling message
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ScreenShareSignal {
    /// Start screen share request
    #[serde(rename = "start")]
    Start {
        /// Session ID
        session_id: String,
    },

    /// Stop screen share request
    #[serde(rename = "stop")]
    Stop {
        /// Session ID
        session_id: String,
    },

    /// WebRTC SDP offer
    #[serde(rename = "offer")]
    Offer {
        /// Session ID
        session_id: String,
        /// SDP offer content
        sdp: String,
    },

    /// WebRTC SDP answer
    #[serde(rename = "answer")]
    Answer {
        /// Session ID
        session_id: String,
        /// SDP answer content
        sdp: String,
    },

    /// ICE candidate
    #[serde(rename = "ice-candidate")]
    IceCandidate {
        /// Session ID
        session_id: String,
        /// ICE candidate string
        candidate: String,
        /// SDP mid
        sdp_mid: Option<String>,
        /// SDP m-line index
        sdp_m_line_index: Option<u32>,
    },

    /// Screen share status update
    #[serde(rename = "status")]
    Status {
        /// Session ID
        session_id: String,
        /// Status: "active", "paused", "ended"
        status: String,
    },
}

impl ScreenShareSignal {
    /// Parse from JSON bytes
    pub fn from_bytes(data: &[u8]) -> Result<Self, MqttError> {
        serde_json::from_slice(data)
            .map_err(|e| MqttError::MessageParse(format!("Invalid screen share signal: {}", e)))
    }

    /// Serialize to JSON bytes
    pub fn to_bytes(&self) -> Result<Vec<u8>, MqttError> {
        serde_json::to_vec(self)
            .map_err(|e| MqttError::MessageParse(format!("Failed to serialize signal: {}", e)))
    }

    /// Get session ID from any signal type
    pub fn session_id(&self) -> &str {
        match self {
            Self::Start { session_id } => session_id,
            Self::Stop { session_id } => session_id,
            Self::Offer { session_id, .. } => session_id,
            Self::Answer { session_id, .. } => session_id,
            Self::IceCandidate { session_id, .. } => session_id,
            Self::Status { session_id, .. } => session_id,
        }
    }
}

/// Screen share session state
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionState {
    /// Waiting for offer
    AwaitingOffer,
    /// Offer received, waiting for answer
    OfferReceived,
    /// Answer sent, exchanging ICE candidates
    Connecting,
    /// Screen share active
    Active,
    /// Session ended
    Ended,
}

/// Screen share session manager
pub struct ScreenShareSession {
    /// Session ID
    pub session_id: String,
    /// Current state
    pub state: SessionState,
    /// Local SDP (offer or answer)
    pub local_sdp: Option<String>,
    /// Remote SDP (offer or answer)
    pub remote_sdp: Option<String>,
    /// Collected ICE candidates
    pub ice_candidates: Vec<String>,
    /// Topic for this session
    /// Topic for this session (currently unused, but retained for future use)
    #[allow(dead_code)]
    topic: Topic,
}

impl ScreenShareSession {
    /// Create new session
    pub fn new(session_id: impl Into<String>, user_id: &str, client_id: &str) -> Self {
        let session_id = session_id.into();
        Self {
            session_id,
            state: SessionState::AwaitingOffer,
            local_sdp: None,
            remote_sdp: None,
            ice_candidates: Vec::new(),
            topic: Topic::screen_share(user_id, client_id),
        }
    }

    /// Handle incoming signal
    pub fn handle_signal(&mut self, signal: &ScreenShareSignal) {
        match signal {
            ScreenShareSignal::Offer { sdp, .. } => {
                debug!(session_id = %self.session_id, "Received offer");
                self.remote_sdp = Some(sdp.clone());
                self.state = SessionState::OfferReceived;
            }
            ScreenShareSignal::Answer { sdp, .. } => {
                debug!(session_id = %self.session_id, "Received answer");
                self.remote_sdp = Some(sdp.clone());
                self.state = SessionState::Connecting;
            }
            ScreenShareSignal::IceCandidate { candidate, .. } => {
                debug!(session_id = %self.session_id, "Received ICE candidate");
                self.ice_candidates.push(candidate.clone());
            }
            ScreenShareSignal::Status { status, .. } => {
                debug!(session_id = %self.session_id, status = %status, "Status update");
                match status.as_str() {
                    "active" => self.state = SessionState::Active,
                    "ended" => self.state = SessionState::Ended,
                    _ => {}
                }
            }
            ScreenShareSignal::Stop { .. } => {
                info!(session_id = %self.session_id, "Session stopped");
                self.state = SessionState::Ended;
            }
            _ => {}
        }
    }
}

/// Screen share client extension for MqttClient
pub struct ScreenShareClient<'a> {
    client: &'a MqttClient,
    user_id: String,
    client_id: String,
}

impl<'a> ScreenShareClient<'a> {
    /// Create screen share client wrapper
    pub fn new(client: &'a MqttClient) -> Self {
        Self {
            user_id: client.user_id().to_string(),
            client_id: client.client_id().to_string(),
            client,
        }
    }

    /// Subscribe to screen share topic
    pub async fn subscribe(&self) -> Result<(), MqttError> {
        let topic = Topic::screen_share(&self.user_id, &self.client_id);
        self.client.subscribe(&topic).await
    }

    /// Send start screen share request
    pub async fn start(&self, session_id: &str) -> Result<(), MqttError> {
        let signal = ScreenShareSignal::Start {
            session_id: session_id.to_string(),
        };
        self.send_signal(&signal).await
    }

    /// Send stop screen share request
    pub async fn stop(&self, session_id: &str) -> Result<(), MqttError> {
        let signal = ScreenShareSignal::Stop {
            session_id: session_id.to_string(),
        };
        self.send_signal(&signal).await
    }

    /// Send WebRTC offer
    pub async fn send_offer(&self, session_id: &str, sdp: &str) -> Result<(), MqttError> {
        let signal = ScreenShareSignal::Offer {
            session_id: session_id.to_string(),
            sdp: sdp.to_string(),
        };
        self.send_signal(&signal).await
    }

    /// Send WebRTC answer
    pub async fn send_answer(&self, session_id: &str, sdp: &str) -> Result<(), MqttError> {
        let signal = ScreenShareSignal::Answer {
            session_id: session_id.to_string(),
            sdp: sdp.to_string(),
        };
        self.send_signal(&signal).await
    }

    /// Send ICE candidate
    pub async fn send_ice_candidate(
        &self,
        session_id: &str,
        candidate: &str,
        sdp_mid: Option<&str>,
        sdp_m_line_index: Option<u32>,
    ) -> Result<(), MqttError> {
        let signal = ScreenShareSignal::IceCandidate {
            session_id: session_id.to_string(),
            candidate: candidate.to_string(),
            sdp_mid: sdp_mid.map(String::from),
            sdp_m_line_index,
        };
        self.send_signal(&signal).await
    }

    /// Send signal to screen share topic
    async fn send_signal(&self, signal: &ScreenShareSignal) -> Result<(), MqttError> {
        let topic = Topic::screen_share(&self.user_id, &self.client_id);
        let payload = signal.to_bytes()?;
        self.client.publish(&topic, &payload).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signal_serialize() {
        let signal = ScreenShareSignal::Offer {
            session_id: "sess-123".to_string(),
            sdp: "v=0\r\n...".to_string(),
        };

        let bytes = signal.to_bytes().unwrap();
        let parsed = ScreenShareSignal::from_bytes(&bytes).unwrap();

        match parsed {
            ScreenShareSignal::Offer { session_id, sdp } => {
                assert_eq!(session_id, "sess-123");
                assert_eq!(sdp, "v=0\r\n...");
            }
            _ => panic!("Wrong signal type"),
        }
    }

    #[test]
    fn test_session_state_machine() {
        let mut session = ScreenShareSession::new("test", "user-1", "client-1");
        assert_eq!(session.state, SessionState::AwaitingOffer);

        session.handle_signal(&ScreenShareSignal::Offer {
            session_id: "test".to_string(),
            sdp: "offer".to_string(),
        });
        assert_eq!(session.state, SessionState::OfferReceived);

        session.handle_signal(&ScreenShareSignal::IceCandidate {
            session_id: "test".to_string(),
            candidate: "candidate:1".to_string(),
            sdp_mid: None,
            sdp_m_line_index: None,
        });
        assert_eq!(session.ice_candidates.len(), 1);

        session.handle_signal(&ScreenShareSignal::Status {
            session_id: "test".to_string(),
            status: "active".to_string(),
        });
        assert_eq!(session.state, SessionState::Active);

        session.handle_signal(&ScreenShareSignal::Stop {
            session_id: "test".to_string(),
        });
        assert_eq!(session.state, SessionState::Ended);
    }
}
