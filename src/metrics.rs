use std::collections::VecDeque;
use std::time::Duration;

/// Tracks session-level RTT samples, flush statistics, and prediction accuracy.
#[derive(Debug)]
pub struct SessionMetrics {
    rtt_samples: VecDeque<Duration>,
    rtt_capacity: usize,
    total_bytes_sent: u64,
    total_flushes: u64,
    bytes_saved: u64,
    prediction_hits: u64,
    prediction_misses: u64,
    current_buffer_depth: usize,
}

impl SessionMetrics {
    pub fn new(rtt_capacity: usize) -> Self {
        Self {
            rtt_samples: VecDeque::with_capacity(rtt_capacity),
            rtt_capacity,
            total_bytes_sent: 0,
            total_flushes: 0,
            bytes_saved: 0,
            prediction_hits: 0,
            prediction_misses: 0,
            current_buffer_depth: 0,
        }
    }

    /// Record an observed RTT sample; evicts the oldest when at capacity.
    pub fn record_rtt(&mut self, rtt: Duration) {
        if self.rtt_samples.len() == self.rtt_capacity {
            self.rtt_samples.pop_front();
        }
        self.rtt_samples.push_back(rtt);
    }

    /// Record a buffer flush. `batch_size` bytes were sent in one round-trip.
    /// Saves (batch_size - 1) vs sending each byte individually.
    pub fn record_flush(&mut self, batch_size: usize) {
        self.total_bytes_sent += batch_size as u64;
        self.total_flushes += 1;
        if batch_size > 1 {
            self.bytes_saved += (batch_size as u64) - 1;
        }
        self.current_buffer_depth = 0;
    }

    /// Rolling average of recent RTT samples; `Duration::ZERO` when no data.
    pub fn rtt_estimate(&self) -> Duration {
        if self.rtt_samples.is_empty() {
            return Duration::ZERO;
        }
        let sum: Duration = self.rtt_samples.iter().sum();
        sum / self.rtt_samples.len() as u32
    }

    pub fn bytes_saved(&self) -> u64 {
        self.bytes_saved
    }

    pub fn total_flushes(&self) -> u64 {
        self.total_flushes
    }

    pub fn total_bytes_sent(&self) -> u64 {
        self.total_bytes_sent
    }

    /// Fraction of correct predictions; 1.0 when no prediction data (vacuously perfect).
    pub fn prediction_accuracy(&self) -> f64 {
        let total = self.prediction_hits + self.prediction_misses;
        if total == 0 {
            1.0
        } else {
            self.prediction_hits as f64 / total as f64
        }
    }

    pub fn set_buffer_depth(&mut self, depth: usize) {
        self.current_buffer_depth = depth;
    }

    pub fn buffer_depth(&self) -> usize {
        self.current_buffer_depth
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rtt_estimate_on_empty_returns_zero() {
        let m = SessionMetrics::new(8);
        assert_eq!(m.rtt_estimate(), Duration::ZERO);
    }

    #[test]
    fn rtt_estimate_averages_samples() {
        let mut m = SessionMetrics::new(8);
        m.record_rtt(Duration::from_millis(100));
        m.record_rtt(Duration::from_millis(200));
        m.record_rtt(Duration::from_millis(300));
        assert_eq!(m.rtt_estimate(), Duration::from_millis(200));
    }

    #[test]
    fn rtt_ring_buffer_evicts_oldest() {
        let mut m = SessionMetrics::new(3);
        m.record_rtt(Duration::from_millis(100));
        m.record_rtt(Duration::from_millis(200));
        m.record_rtt(Duration::from_millis(300));
        // 4th push evicts 100ms; ring now holds 200, 300, 400
        m.record_rtt(Duration::from_millis(400));
        assert_eq!(m.rtt_estimate(), Duration::from_millis(300));
    }

    #[test]
    fn record_flush_accumulates_bytes_saved() {
        let mut m = SessionMetrics::new(8);
        m.record_flush(3); // saves 2
        assert_eq!(m.bytes_saved(), 2);
        m.record_flush(5); // saves 4
        assert_eq!(m.bytes_saved(), 6);
    }

    #[test]
    fn bytes_saved_zero_when_all_flushed_singly() {
        let mut m = SessionMetrics::new(8);
        m.record_flush(1);
        m.record_flush(1);
        m.record_flush(1);
        assert_eq!(m.bytes_saved(), 0);
    }

    #[test]
    fn prediction_accuracy_vacuously_perfect_when_empty() {
        let m = SessionMetrics::new(8);
        assert_eq!(m.prediction_accuracy(), 1.0);
    }

    #[test]
    fn buffer_depth_tracks_pending() {
        let mut m = SessionMetrics::new(8);
        m.set_buffer_depth(42);
        assert_eq!(m.buffer_depth(), 42);
    }

    #[test]
    fn buffer_depth_zero_after_flush() {
        let mut m = SessionMetrics::new(8);
        m.set_buffer_depth(100);
        m.record_flush(100);
        assert_eq!(m.buffer_depth(), 0);
    }
}
