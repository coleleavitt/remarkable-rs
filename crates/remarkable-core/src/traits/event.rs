//! Event and notification system traits
//!
//! This module provides traits for subscribing to and receiving
//! sync events, document changes, and real-time notifications.

use std::pin::Pin;
use std::future::Future;

/// Change type for document events
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    /// Document created
    Created,
    /// Document modified
    Modified,
    /// Document deleted
    Deleted,
    /// Document moved
    Moved,
    /// Document renamed
    Renamed,
}

/// Sync event types
#[derive(Debug, Clone)]
pub enum SyncEvent {
    /// Sync operation started
    Started {
        /// Sync ID for tracking
        sync_id: String,
    },
    
    /// Document changed
    DocumentChanged {
        /// Document ID
        id: String,
        /// Type of change
        kind: ChangeKind,
        /// New version (if applicable)
        version: Option<u32>,
    },
    
    /// Sync completed successfully
    Completed {
        /// Sync ID
        sync_id: String,
        /// New generation number
        generation: u64,
        /// Number of changes
        changes: usize,
    },
    
    /// Conflict detected
    ConflictDetected {
        /// Document ID
        id: String,
        /// Local version
        local_version: u32,
        /// Server version
        server_version: u32,
    },
    
    /// Sync failed
    Failed {
        /// Sync ID
        sync_id: String,
        /// Error message
        error: String,
    },
    
    /// Progress update
    Progress {
        /// Sync ID
        sync_id: String,
        /// Current progress (0.0 - 1.0)
        progress: f32,
        /// Current operation description
        message: String,
    },
}

impl SyncEvent {
    /// Get the sync ID if present
    pub fn sync_id(&self) -> Option<&str> {
        match self {
            Self::Started { sync_id } => Some(sync_id),
            Self::Completed { sync_id, .. } => Some(sync_id),
            Self::Failed { sync_id, .. } => Some(sync_id),
            Self::Progress { sync_id, .. } => Some(sync_id),
            _ => None,
        }
    }
    
    /// Get the document ID if present
    pub fn document_id(&self) -> Option<&str> {
        match self {
            Self::DocumentChanged { id, .. } => Some(id),
            Self::ConflictDetected { id, .. } => Some(id),
            _ => None,
        }
    }
}

/// Event stream type alias
pub type EventStream<T> = Pin<Box<dyn futures::Stream<Item = T> + Send>>;

/// Event error type
#[derive(Debug, thiserror::Error)]
pub enum EventError {
    #[error("subscription failed: {0}")]
    SubscriptionFailed(String),
    
    #[error("disconnected")]
    Disconnected,
    
    #[error("timeout")]
    Timeout,
}

/// Event source trait
///
/// Implementations provide real-time event streams for sync
/// and document changes.
pub trait EventSource: Send + Sync {
    /// Event type emitted by this source
    type Event;
    
    /// Subscribe to events
    fn subscribe(&self) -> impl Future<Output = Result<EventStream<Self::Event>, EventError>> + Send;
    
    /// Unsubscribe from events
    fn unsubscribe(&self) -> impl Future<Output = Result<(), EventError>> + Send;
    
    /// Check if currently subscribed
    fn is_subscribed(&self) -> bool;
}

/// MQTT-specific event configuration
#[derive(Debug, Clone)]
pub struct MqttEventConfig {
    /// Broker URL
    pub broker_url: String,
    /// User topic prefix
    pub topic_prefix: String,
    /// Keep-alive interval in seconds
    pub keep_alive_secs: u64,
}

impl Default for MqttEventConfig {
    fn default() -> Self {
        Self {
            broker_url: "wss://broker.remarkable.com".to_string(),
            topic_prefix: "user".to_string(),
            keep_alive_secs: 60,
        }
    }
}

/// Event handler trait for callbacks
pub trait EventHandler<E>: Send + Sync {
    /// Handle an event
    fn handle(&self, event: E);
    
    /// Handle an error
    fn on_error(&self, error: EventError) {
        let _ = error; // Default: ignore
    }
}

/// Simple function-based event handler
pub struct FnHandler<F> {
    handler: F,
}

impl<F, E> EventHandler<E> for FnHandler<F>
where
    F: Fn(E) + Send + Sync,
{
    fn handle(&self, event: E) {
        (self.handler)(event);
    }
}

/// Create a handler from a function
pub fn handler<F, E>(f: F) -> FnHandler<F>
where
    F: Fn(E) + Send + Sync,
{
    FnHandler { handler: f }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_sync_event_accessors() {
        let event = SyncEvent::DocumentChanged {
            id: "doc-123".to_string(),
            kind: ChangeKind::Modified,
            version: Some(5),
        };
        
        assert_eq!(event.document_id(), Some("doc-123"));
        assert_eq!(event.sync_id(), None);
        
        let started = SyncEvent::Started {
            sync_id: "sync-456".to_string(),
        };
        
        assert_eq!(started.sync_id(), Some("sync-456"));
        assert_eq!(started.document_id(), None);
    }
}
