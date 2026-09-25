//! MQTT topic structure for reMarkable sync
//!
//! # Topic Hierarchy
//!
//! ```text
//! user/{user_id}/
//! ├── sync                              # User-wide sync events
//! └── client/{client_id}/
//!     ├── notifications                 # General notifications
//!     ├── sync                          # Client-specific sync
//!     └── signaling/...                 # Screen share broker replies
//!
//! remarkable/screenshare/signaling/user/{user_id}/client/{client_id}
//!                                       # Screen share requests (publish)
//! ```
//!
//! # Message Flow
//!
//! 1. Device completes sync → publishes to `user/{user_id}/sync`
//! 2. Other clients subscribed → receive sync notification
//! 3. Clients fetch updated root hash → download changes

use std::fmt;

/// MQTT topic for reMarkable notifications
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Topic {
    /// User-wide sync events: `user/{user_id}/sync`
    UserSync { user_id: String },

    /// Client notifications: `user/{user_id}/client/{client_id}/notifications`
    ClientNotifications { user_id: String, client_id: String },

    /// Client sync events: `user/{user_id}/client/{client_id}/sync`
    ClientSync { user_id: String, client_id: String },

    /// Screen share signaling requests:
    /// `remarkable/screenshare/signaling/user/{user_id}/client/{client_id}`
    ScreenShareSignaling { user_id: String, client_id: String },

    /// Wildcard subscription for all client topics
    AllClientTopics { user_id: String, client_id: String },

    /// Custom topic
    Custom(String),
}

impl Topic {
    /// Create user sync topic
    pub fn user_sync(user_id: impl Into<String>) -> Self {
        Self::UserSync {
            user_id: user_id.into(),
        }
    }

    /// Create client notifications topic
    pub fn client_notifications(user_id: impl Into<String>, client_id: impl Into<String>) -> Self {
        Self::ClientNotifications {
            user_id: user_id.into(),
            client_id: client_id.into(),
        }
    }

    /// Create client sync topic
    pub fn client_sync(user_id: impl Into<String>, client_id: impl Into<String>) -> Self {
        Self::ClientSync {
            user_id: user_id.into(),
            client_id: client_id.into(),
        }
    }

    /// Create the screen share signaling topic a client publishes requests to
    pub fn screen_share_signaling(user_id: impl Into<String>, client_id: impl Into<String>) -> Self {
        Self::ScreenShareSignaling {
            user_id: user_id.into(),
            client_id: client_id.into(),
        }
    }

    /// Create wildcard subscription for all client topics
    pub fn all_client(user_id: impl Into<String>, client_id: impl Into<String>) -> Self {
        Self::AllClientTopics {
            user_id: user_id.into(),
            client_id: client_id.into(),
        }
    }

    /// Get the MQTT topic string
    pub fn as_str(&self) -> String {
        match self {
            Self::UserSync { user_id } => format!("user/{}/sync", user_id),
            Self::ClientNotifications { user_id, client_id } => {
                format!("user/{}/client/{}/notifications", user_id, client_id)
            }
            Self::ClientSync { user_id, client_id } => {
                format!("user/{}/client/{}/sync", user_id, client_id)
            }
            Self::ScreenShareSignaling { user_id, client_id } => {
                crate::screenshare::signaling_topic(user_id, client_id)
            }
            Self::AllClientTopics { user_id, client_id } => {
                format!("user/{}/client/{}/#", user_id, client_id)
            }
            Self::Custom(s) => s.clone(),
        }
    }

    /// Parse topic string back to Topic enum
    pub fn parse(topic: &str) -> Option<Self> {
        let parts: Vec<&str> = topic.split('/').collect();

        match parts.as_slice() {
            ["user", user_id, "sync"] => Some(Self::UserSync {
                user_id: (*user_id).to_string(),
            }),
            ["user", user_id, "client", client_id, "notifications"] => {
                Some(Self::ClientNotifications {
                    user_id: (*user_id).to_string(),
                    client_id: (*client_id).to_string(),
                })
            }
            ["user", user_id, "client", client_id, "sync"] => Some(Self::ClientSync {
                user_id: (*user_id).to_string(),
                client_id: (*client_id).to_string(),
            }),
            ["remarkable", "screenshare", "signaling", "user", user_id, "client", client_id] => {
                Some(Self::ScreenShareSignaling {
                    user_id: (*user_id).to_string(),
                    client_id: (*client_id).to_string(),
                })
            }
            _ => Some(Self::Custom(topic.to_string())),
        }
    }
}

impl fmt::Display for Topic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Standard topics to subscribe for a client
pub fn default_subscriptions(user_id: &str, client_id: &str) -> Vec<Topic> {
    vec![
        Topic::user_sync(user_id),
        Topic::client_notifications(user_id, client_id),
        Topic::client_sync(user_id, client_id),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_topic_strings() {
        let user_sync = Topic::user_sync("user-123");
        assert_eq!(user_sync.as_str(), "user/user-123/sync");

        let client_notif = Topic::client_notifications("user-123", "client-456");
        assert_eq!(
            client_notif.as_str(),
            "user/user-123/client/client-456/notifications"
        );
    }

    #[test]
    fn test_topic_parse() {
        let topic = Topic::parse("user/abc/sync").unwrap();
        assert!(matches!(topic, Topic::UserSync { user_id } if user_id == "abc"));

        let topic = Topic::parse("user/abc/client/xyz/notifications").unwrap();
        assert!(matches!(
            topic,
            Topic::ClientNotifications { user_id, client_id }
            if user_id == "abc" && client_id == "xyz"
        ));
    }

    #[test]
    fn test_screen_share_signaling_topic_round_trips() {
        let topic = Topic::screen_share_signaling("abc", "xyz");
        assert_eq!(
            topic.as_str(),
            "remarkable/screenshare/signaling/user/abc/client/xyz"
        );
        assert_eq!(Topic::parse(&topic.as_str()), Some(topic));
    }

    #[test]
    fn test_default_subscriptions() {
        let subs = default_subscriptions("user-1", "client-1");
        assert_eq!(subs.len(), 3);
    }
}
