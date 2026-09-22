//! Integration with sync events
//!
//! Provides reactive sync based on MQTT notifications.
//!
//! # Workflow
//!
//! 1. Subscribe to sync notifications
//! 2. Receive `sync-complete for generation N` message
//! 3. Trigger sync client to fetch latest root hash
//! 4. Download changed files
//!
//! # Example
//!
//! ```ignore
//! use remarkable_mqtt::sync_events::{SyncEventStream, SyncAction};
//! use remarkable_mqtt::MqttConfig;
//!
//! let config = MqttConfig::from_tokens(&device_token, &user_token)?;
//! let (mut stream, rx) = SyncEventStream::new(config);
//!
//! // Run stream (must be on same thread due to EventLoop constraints)
//! stream.run().await?;
//! ```

use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::{MqttConfig, MqttEvent, ReconnectingClient, MqttError, SyncComplete};

/// Action to take when sync event received
#[derive(Debug, Clone)]
pub enum SyncAction {
    /// Fetch new root hash (generation changed)
    FetchRoot {
        /// New generation number
        generation: u64,
        /// Source device that triggered sync
        source_device: String,
    },

    /// Incremental sync (known changes)
    IncrementalSync {
        /// Changed document IDs
        changed_docs: Vec<String>,
    },

    /// Full resync needed
    FullResync {
        /// Reason for full resync
        reason: String,
    },
}

/// Sync event received from MQTT
#[derive(Debug, Clone)]
pub struct SyncEvent {
    /// Timestamp when event received
    pub timestamp: Instant,
    /// Sync completion details
    pub sync: SyncComplete,
    /// Recommended action
    pub action: SyncAction,
}

/// Tracks sync state across events
pub struct SyncState {
    /// Last known generation
    pub last_generation: Option<u64>,
    /// Pending generations to process
    pub pending: Vec<u64>,
    /// Generation history
    pub history: Vec<(Instant, u64)>,
    /// Debounce window
    pub debounce: Duration,
    /// Last event timestamp
    pub last_event: Option<Instant>,
}

impl Default for SyncState {
    fn default() -> Self {
        Self {
            last_generation: None,
            pending: Vec::new(),
            history: Vec::new(),
            debounce: Duration::from_millis(500),
            last_event: None,
        }
    }
}

impl SyncState {
    /// Create with custom debounce window
    pub fn with_debounce(mut self, debounce: Duration) -> Self {
        self.debounce = debounce;
        self
    }

    /// Process sync complete event
    ///
    /// Returns action to take, if any (handles debouncing)
    pub fn process_event(&mut self, sync: &SyncComplete) -> Option<SyncAction> {
        let now = Instant::now();

        // Check debounce
        if let Some(last) = self.last_event {
            if now.duration_since(last) < self.debounce {
                debug!(
                    generation = sync.generation,
                    "Debouncing sync event"
                );
                self.pending.push(sync.generation);
                return None;
            }
        }

        self.last_event = Some(now);
        self.history.push((now, sync.generation));

        // Trim history to last 100 events
        if self.history.len() > 100 {
            self.history.drain(0..50);
        }

        // Check if generation actually changed
        if let Some(last_gen) = self.last_generation {
            if sync.generation <= last_gen {
                debug!(
                    current = sync.generation,
                    last = last_gen,
                    "Stale generation, ignoring"
                );
                return None;
            }

            // Check for gaps
            if sync.generation > last_gen + 1 {
                warn!(
                    current = sync.generation,
                    last = last_gen,
                    gap = sync.generation - last_gen - 1,
                    "Generation gap detected"
                );
            }
        }

        self.last_generation = Some(sync.generation);

        // Include any pending generations
        let mut all_generations = std::mem::take(&mut self.pending);
        all_generations.push(sync.generation);
        all_generations.sort();
        all_generations.dedup();

        Some(SyncAction::FetchRoot {
            generation: sync.generation,
            source_device: sync.source_device_id.clone(),
        })
    }

    /// Get sync statistics
    pub fn stats(&self) -> SyncStats {
        let events_last_minute = self
            .history
            .iter()
            .filter(|(t, _)| t.elapsed() < Duration::from_secs(60))
            .count();

        let events_last_hour = self
            .history
            .iter()
            .filter(|(t, _)| t.elapsed() < Duration::from_secs(3600))
            .count();

        SyncStats {
            last_generation: self.last_generation,
            total_events: self.history.len(),
            events_last_minute,
            events_last_hour,
            pending_count: self.pending.len(),
        }
    }
}

/// Sync statistics
#[derive(Debug, Clone)]
pub struct SyncStats {
    /// Last processed generation
    pub last_generation: Option<u64>,
    /// Total events processed
    pub total_events: usize,
    /// Events in last minute
    pub events_last_minute: usize,
    /// Events in last hour
    pub events_last_hour: usize,
    /// Pending events (debounced)
    pub pending_count: usize,
}

/// Sync event handler with callback
pub struct SyncEventHandler<F>
where
    F: FnMut(SyncEvent) + Send + 'static,
{
    client: ReconnectingClient,
    state: SyncState,
    handler: F,
}

impl<F> SyncEventHandler<F>
where
    F: FnMut(SyncEvent) + Send + 'static,
{
    /// Create new handler
    pub fn new(config: MqttConfig, handler: F) -> Self {
        Self {
            client: ReconnectingClient::new(config),
            state: SyncState::default(),
            handler,
        }
    }

    /// Create with custom debounce
    pub fn with_debounce(mut self, debounce: Duration) -> Self {
        self.state = self.state.with_debounce(debounce);
        self
    }

    /// Run event loop
    pub async fn run(&mut self) -> Result<(), MqttError> {
        self.client.connect().await?;
        self.client.subscribe_default().await?;

        info!("Sync event handler running");

        loop {
            match self.client.poll().await {
                Ok(MqttEvent::SyncComplete { sync, .. }) => {
                    if let Some(action) = self.state.process_event(&sync) {
                        let event = SyncEvent {
                            timestamp: Instant::now(),
                            sync,
                            action,
                        };
                        (self.handler)(event);
                    }
                }
                Ok(MqttEvent::Disconnected) => {
                    warn!("Disconnected, will reconnect");
                }
                Ok(_) => {
                    // Other events, ignore
                }
                Err(e) => {
                    warn!(?e, "Handler error");
                    if !e.is_recoverable() {
                        return Err(e);
                    }
                }
            }
        }
    }

    /// Get current stats
    pub fn stats(&self) -> SyncStats {
        self.state.stats()
    }
}

/// Channel-based sync event stream
pub struct SyncEventStream {
    client: ReconnectingClient,
    state: SyncState,
    tx: mpsc::Sender<SyncEvent>,
}

impl SyncEventStream {
    /// Create new event stream, returns receiver
    pub fn new(config: MqttConfig) -> (Self, mpsc::Receiver<SyncEvent>) {
        let (tx, rx) = mpsc::channel(100);
        let stream = Self {
            client: ReconnectingClient::new(config),
            state: SyncState::default(),
            tx,
        };
        (stream, rx)
    }

    /// Run the stream
    ///
    /// Note: This must be run on the same thread as creation due to
    /// rumqttc's EventLoop not being Sync.
    pub async fn run(&mut self) -> Result<(), MqttError> {
        self.client.connect().await?;
        self.client.subscribe_default().await?;

        loop {
            match self.client.poll().await {
                Ok(MqttEvent::SyncComplete { sync, .. }) => {
                    if let Some(action) = self.state.process_event(&sync) {
                        let event = SyncEvent {
                            timestamp: Instant::now(),
                            sync,
                            action,
                        };
                        if self.tx.send(event).await.is_err() {
                            info!("Receiver dropped, stopping stream");
                            return Ok(());
                        }
                    }
                }
                Ok(_) => {}
                Err(e) if e.is_recoverable() => continue,
                Err(e) => return Err(e),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_state_tracks_generation() {
        // Use zero debounce for testing
        let mut state = SyncState::default().with_debounce(Duration::ZERO);

        let sync1 = SyncComplete {
            source_device_id: "dev-1".to_string(),
            generation: 10,
        };

        let action = state.process_event(&sync1);
        assert!(action.is_some(), "First event should trigger action");
        assert_eq!(state.last_generation, Some(10));

        // Same generation should be ignored (stale)
        let action = state.process_event(&sync1);
        assert!(action.is_none(), "Same generation should be ignored");

        // Older generation should be ignored
        let sync_old = SyncComplete {
            source_device_id: "dev-1".to_string(),
            generation: 5,
        };
        let action = state.process_event(&sync_old);
        assert!(action.is_none(), "Older generation should be ignored");

        // Newer generation should trigger action
        let sync2 = SyncComplete {
            source_device_id: "dev-1".to_string(),
            generation: 11,
        };
        let action = state.process_event(&sync2);
        assert!(action.is_some(), "Newer generation should trigger action");
        assert_eq!(state.last_generation, Some(11));
    }

    #[test]
    fn test_debounce() {
        // Default debounce of 500ms
        let mut state = SyncState::default();

        let sync1 = SyncComplete {
            source_device_id: "dev-1".to_string(),
            generation: 10,
        };

        // First event should not be debounced
        let action = state.process_event(&sync1);
        assert!(action.is_some());

        // Immediate second event should be debounced (even if different gen)
        let sync2 = SyncComplete {
            source_device_id: "dev-1".to_string(),
            generation: 11,
        };
        let action = state.process_event(&sync2);
        assert!(action.is_none(), "Should be debounced");
        assert_eq!(state.pending.len(), 1, "Should be in pending queue");
    }
}
