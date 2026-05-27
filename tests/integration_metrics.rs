use ptyx::metrics::SessionMetrics;
use std::time::Duration;

#[test]
fn metrics_bytes_saved_nonzero_after_batched_flush() {
    let mut m = SessionMetrics::new(8);
    // Simulate 3 keystrokes batched into a single flush
    m.record_flush(3);
    assert!(
        m.bytes_saved() > 0,
        "batching 3 bytes should save round-trips"
    );
    assert_eq!(m.bytes_saved(), 2); // saved 2 vs sending each byte alone
}

#[test]
fn metrics_rtt_nonzero_after_roundtrip() {
    let mut m = SessionMetrics::new(8);
    assert_eq!(m.rtt_estimate(), Duration::ZERO);
    m.record_rtt(Duration::from_millis(50));
    assert!(m.rtt_estimate() > Duration::ZERO);
}

#[test]
fn metrics_total_bytes_tracks_all_flushes() {
    let mut m = SessionMetrics::new(8);
    m.record_flush(10);
    m.record_flush(5);
    m.record_flush(3);
    assert_eq!(m.total_bytes_sent(), 18);
    assert_eq!(m.total_flushes(), 3);
}

#[test]
fn metrics_rtt_ring_capacity_respected() {
    // Capacity of 4 — a 5th sample evicts the oldest
    let mut m = SessionMetrics::new(4);
    for ms in [10, 20, 30, 40] {
        m.record_rtt(Duration::from_millis(ms));
    }
    assert_eq!(m.rtt_estimate(), Duration::from_millis(25)); // avg(10,20,30,40)

    m.record_rtt(Duration::from_millis(50)); // evicts 10ms
                                             // remaining: 20, 30, 40, 50 → avg = 35ms
    assert_eq!(m.rtt_estimate(), Duration::from_millis(35));
}
