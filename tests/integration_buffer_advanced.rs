use ptyx::buffer::InputBuffer;
use std::time::Duration;

#[test]
fn passthrough_mode_bytes_available_immediately() {
    let mut buf = InputBuffer::new(Duration::from_secs(100), 512);
    buf.set_passthrough(true);
    buf.push(b'x');
    // should_flush must be true despite the very long deadline
    assert!(buf.should_flush());
    assert_eq!(buf.take(), b"x");
}

#[test]
fn raw_mode_output_triggers_passthrough() {
    // Simulate: proxy receives alt-screen entry sequence in PTY output.
    // The detection functions are tested in proxy unit tests.
    // Here we test that after enabling passthrough, the buffer behaves correctly.
    let mut buf = InputBuffer::new(Duration::from_millis(500), 512);
    assert!(!buf.should_flush());
    buf.push(b'a');
    assert!(!buf.should_flush()); // normal mode, deadline not reached

    buf.set_passthrough(true); // proxy calls this on detecting \x1b[?1049h
    buf.push(b'b');
    assert!(buf.should_flush()); // passthrough: flush immediately
}

#[test]
fn backpressure_stops_at_max_size() {
    let mut buf = InputBuffer::new(Duration::from_millis(500), 4);
    buf.push(b'a');
    buf.push(b'b');
    buf.push(b'c');
    assert!(!buf.is_full()); // 3/4 — not full yet
    buf.push(b'd');
    assert!(buf.is_full()); // 4/4 — caller should stop reading stdin
}

#[test]
fn backpressure_releases_after_flush() {
    let mut buf = InputBuffer::new(Duration::from_millis(500), 2);
    buf.push(b'a');
    buf.push(b'b');
    assert!(buf.is_full());
    buf.take();
    assert!(!buf.is_full());
}

#[test]
fn adaptive_interval_applied_after_rtt_sample() {
    let mut buf = InputBuffer::new(Duration::from_millis(20), 512);
    buf.set_adaptive(true);

    // Simulate high-RTT observation (500ms > 200ms threshold)
    // Formula: min(100ms, 500ms * 3/10) = min(100ms, 150ms) = 100ms
    buf.set_adaptive_interval(Duration::from_millis(500));
    assert_eq!(buf.flush_interval(), Duration::from_millis(100));
}

#[test]
fn binary_mode_passes_invalid_utf8_unmodified() {
    let mut buf = InputBuffer::new(Duration::from_millis(20), 512);
    buf.set_binary_mode(true);

    // These bytes are not valid UTF-8 — in binary mode they pass through raw
    let raw: &[u8] = &[0x80, 0xFE, 0xFF, 0x00];
    for &b in raw {
        buf.push(b);
    }
    assert_eq!(buf.take(), raw);
}
