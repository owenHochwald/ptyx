use std::time::Duration;

/// Reconnect behavior for replacing a dead SSH child with a fresh one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistenceConfig {
    pub reconnect: bool,
    pub reconnect_timeout_ms: u64,
    pub reconnect_initial_delay_ms: u64,
    pub reconnect_max_delay_ms: u64,
}

impl Default for PersistenceConfig {
    fn default() -> Self {
        Self {
            reconnect: false,
            reconnect_timeout_ms: 10_000,
            reconnect_initial_delay_ms: 100,
            reconnect_max_delay_ms: 2_000,
        }
    }
}

impl PersistenceConfig {
    pub fn timeout(&self) -> Duration {
        Duration::from_millis(self.reconnect_timeout_ms)
    }

    pub fn initial_delay(&self) -> Duration {
        Duration::from_millis(self.reconnect_initial_delay_ms)
    }

    pub fn max_delay(&self) -> Duration {
        Duration::from_millis(self.reconnect_max_delay_ms)
    }
}

/// Local state that can be replayed after a reconnect.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SessionState {
    pending_input: Vec<u8>,
}

impl SessionState {
    pub fn capture_pending(&mut self, bytes: Vec<u8>) {
        if !bytes.is_empty() {
            self.pending_input.extend(bytes);
        }
    }

    pub fn take_pending(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.pending_input)
    }

    pub fn pending_len(&self) -> usize {
        self.pending_input.len()
    }
}

/// Exponential reconnect backoff capped by the configured maximum delay.
#[derive(Debug, Clone)]
pub struct ReconnectBackoff {
    next: Duration,
    max: Duration,
}

impl ReconnectBackoff {
    pub fn new(initial: Duration, max: Duration) -> Self {
        Self { next: initial, max }
    }

    pub fn next_delay(&mut self) -> Duration {
        let current = self.next.min(self.max);
        self.next = (current * 2).min(self.max);
        current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_reconnect_is_opt_in() {
        assert!(!PersistenceConfig::default().reconnect);
    }

    #[test]
    fn pending_buffer_replayed_on_reconnect() {
        let mut state = SessionState::default();
        state.capture_pending(b"ls ".to_vec());
        state.capture_pending(b"-la\n".to_vec());

        assert_eq!(state.pending_len(), 7);
        assert_eq!(state.take_pending(), b"ls -la\n");
        assert_eq!(state.pending_len(), 0);
    }

    #[test]
    fn reconnect_backoff_doubles_until_cap() {
        let mut backoff =
            ReconnectBackoff::new(Duration::from_millis(100), Duration::from_millis(250));

        assert_eq!(backoff.next_delay(), Duration::from_millis(100));
        assert_eq!(backoff.next_delay(), Duration::from_millis(200));
        assert_eq!(backoff.next_delay(), Duration::from_millis(250));
        assert_eq!(backoff.next_delay(), Duration::from_millis(250));
    }
}
