//! Auto-reconnection with exponential backoff
//!
//! Provides resilient MQTT connection with automatic recovery.
//!
//! # Backoff Strategy
//!
//! - Initial delay: 1 second
//! - Max delay: 60 seconds
//! - Multiplier: 2x per failure
//! - Jitter: ±20%
//!
//! # Example
//!
//! ```ignore
//! use remarkable_mqtt::{MqttConfig, ReconnectingClient};
//!
//! let config = MqttConfig::from_tokens(&device_token, &user_token)?;
//! let mut client = ReconnectingClient::new(config);
//!
//! // Client automatically reconnects on failure
//! loop {
//!     match client.poll().await {
//!         Ok(event) => handle_event(event),
//!         Err(e) if e.is_recoverable() => continue, // Auto-retry
//!         Err(e) => return Err(e),
//!     }
//! }
//! ```

use std::time::Duration;
use tokio::time::sleep;
use tracing::{error, info, warn};

use crate::{MqttClient, MqttConfig, MqttError, MqttEvent};

/// Backoff configuration
#[derive(Debug, Clone)]
pub struct BackoffConfig {
    /// Initial delay before first retry
    pub initial_delay: Duration,
    /// Maximum delay between retries
    pub max_delay: Duration,
    /// Multiplier applied after each failure
    pub multiplier: f64,
    /// Maximum number of retries (None = unlimited)
    pub max_retries: Option<u32>,
    /// Jitter factor (0.0 - 1.0)
    pub jitter: f64,
}

impl Default for BackoffConfig {
    fn default() -> Self {
        Self {
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
            multiplier: 2.0,
            max_retries: None,
            jitter: 0.2,
        }
    }
}

impl BackoffConfig {
    /// Calculate delay for given attempt number
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let base_secs = self.initial_delay.as_secs_f64() * self.multiplier.powi(attempt as i32);
        let capped_secs = base_secs.min(self.max_delay.as_secs_f64());

        // Apply jitter
        let jitter_range = capped_secs * self.jitter;
        let jitter = (rand_simple() * 2.0 - 1.0) * jitter_range;
        let final_secs = (capped_secs + jitter).max(0.1);

        Duration::from_secs_f64(final_secs)
    }
}

/// Simple deterministic "random" for jitter (avoids rand dependency)
fn rand_simple() -> f64 {
    use std::time::SystemTime;
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    (nanos % 1000) as f64 / 1000.0
}

/// Connection state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// Not connected
    Disconnected,
    /// Attempting to connect
    Connecting,
    /// Successfully connected
    Connected,
    /// Connection lost, will retry
    Reconnecting { attempt: u32 },
}

/// MQTT client with automatic reconnection
pub struct ReconnectingClient {
    config: MqttConfig,
    backoff: BackoffConfig,
    inner: Option<MqttClient>,
    state: ConnectionState,
    consecutive_failures: u32,
}

impl ReconnectingClient {
    /// Create new reconnecting client
    pub fn new(config: MqttConfig) -> Self {
        Self {
            config,
            backoff: BackoffConfig::default(),
            inner: None,
            state: ConnectionState::Disconnected,
            consecutive_failures: 0,
        }
    }

    /// Create with custom backoff configuration
    pub fn with_backoff(mut self, backoff: BackoffConfig) -> Self {
        self.backoff = backoff;
        self
    }

    /// Get current connection state
    pub fn state(&self) -> ConnectionState {
        self.state
    }

    /// Check if connected
    pub fn is_connected(&self) -> bool {
        matches!(self.state, ConnectionState::Connected)
    }

    /// Connect to broker with auto-retry
    pub async fn connect(&mut self) -> Result<(), MqttError> {
        self.state = ConnectionState::Connecting;

        loop {
            let mut client = MqttClient::new(self.config.clone());

            match client.connect().await {
                Ok(()) => {
                    info!("Connected to MQTT broker");
                    self.inner = Some(client);
                    self.state = ConnectionState::Connected;
                    self.consecutive_failures = 0;
                    return Ok(());
                }
                Err(e) => {
                    self.consecutive_failures += 1;
                    let attempt = self.consecutive_failures;

                    // Check max retries
                    if let Some(max) = self.backoff.max_retries {
                        if attempt >= max {
                            error!(
                                attempt,
                                max_retries = max,
                                error = %e,
                                "Max retries exceeded"
                            );
                            self.state = ConnectionState::Disconnected;
                            return Err(e);
                        }
                    }

                    let delay = self.backoff.delay_for_attempt(attempt);
                    warn!(
                        attempt,
                        delay_secs = delay.as_secs_f64(),
                        error = %e,
                        "Connection failed, retrying"
                    );

                    self.state = ConnectionState::Reconnecting { attempt };
                    sleep(delay).await;
                }
            }
        }
    }

    /// Subscribe to default topics
    pub async fn subscribe_default(&self) -> Result<(), MqttError> {
        let client = self.inner.as_ref().ok_or(MqttError::Disconnected)?;
        client.subscribe_default().await
    }

    /// Poll for next event with auto-reconnect
    pub async fn poll(&mut self) -> Result<MqttEvent, MqttError> {
        // Ensure connected
        if self.inner.is_none() {
            self.connect().await?;
            self.subscribe_default().await?;
        }

        loop {
            let result = {
                let client = self.inner.as_mut().ok_or(MqttError::Disconnected)?;
                client.poll().await
            };

            match result {
                Ok(event) => {
                    self.consecutive_failures = 0;
                    return Ok(event);
                }
                Err(e) if e.is_recoverable() => {
                    self.consecutive_failures += 1;
                    let attempt = self.consecutive_failures;

                    if let Some(max) = self.backoff.max_retries {
                        if attempt >= max {
                            return Err(e);
                        }
                    }

                    let delay = self.backoff.delay_for_attempt(attempt);
                    warn!(
                        attempt,
                        delay_secs = delay.as_secs_f64(),
                        error = %e,
                        "Poll failed, reconnecting"
                    );

                    self.state = ConnectionState::Reconnecting { attempt };
                    self.inner = None;
                    sleep(delay).await;

                    // Reconnect
                    self.connect().await?;
                    self.subscribe_default().await?;
                }
                Err(e) => {
                    error!(?e, "Non-recoverable error");
                    self.state = ConnectionState::Disconnected;
                    self.inner = None;
                    return Err(e);
                }
            }
        }
    }

    /// Disconnect from broker
    pub async fn disconnect(&mut self) -> Result<(), MqttError> {
        if let Some(client) = &self.inner {
            client.disconnect().await?;
        }
        self.inner = None;
        self.state = ConnectionState::Disconnected;
        Ok(())
    }

    /// Get user ID
    pub fn user_id(&self) -> &str {
        &self.config.user_id
    }

    /// Get client ID  
    pub fn client_id(&self) -> &str {
        &self.config.client_id
    }
}

/// Callback-based event handler
pub struct EventHandler<F>
where
    F: Fn(MqttEvent) + Send + Sync,
{
    client: ReconnectingClient,
    handler: F,
}

impl<F> EventHandler<F>
where
    F: Fn(MqttEvent) + Send + Sync,
{
    /// Create event handler
    pub fn new(client: ReconnectingClient, handler: F) -> Self {
        Self { client, handler }
    }

    /// Run event loop
    pub async fn run(&mut self) -> Result<(), MqttError> {
        self.client.connect().await?;
        self.client.subscribe_default().await?;

        loop {
            match self.client.poll().await {
                Ok(event) => {
                    (self.handler)(event);
                }
                Err(e) => {
                    error!(?e, "Event handler error");
                    return Err(e);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backoff_delays() {
        let config = BackoffConfig {
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
            multiplier: 2.0,
            max_retries: None,
            jitter: 0.0, // Disable jitter for predictable tests
        };

        // Attempt 0: 1s
        let d0 = config.delay_for_attempt(0);
        assert!(d0.as_secs_f64() >= 0.9 && d0.as_secs_f64() <= 1.1);

        // Attempt 1: 2s
        let d1 = config.delay_for_attempt(1);
        assert!(d1.as_secs_f64() >= 1.9 && d1.as_secs_f64() <= 2.1);

        // Attempt 5: 32s
        let d5 = config.delay_for_attempt(5);
        assert!(d5.as_secs_f64() >= 31.0 && d5.as_secs_f64() <= 33.0);

        // Attempt 10: capped at 60s
        let d10 = config.delay_for_attempt(10);
        assert!(d10.as_secs_f64() <= 61.0);
    }
}
