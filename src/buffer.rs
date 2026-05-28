use std::time::{Duration, Instant};

/// Batches keystrokes for up to `flush_interval`, flushing immediately on command boundaries.
#[derive(Debug)]
pub struct InputBuffer {
    data: Vec<u8>,
    deadline: Instant,
    flush_interval: Duration,
    max_size: usize,
    /// Partial multi-byte UTF-8 carry-over; never included in `data`.
    utf8_carry: Vec<u8>,
    /// Raw/alt-screen mode: every push is immediately available via take().
    passthrough: bool,
    /// Binary protocol (scp/rsync): skip UTF-8 boundary checking.
    binary_mode: bool,
    /// Enable RTT-based adaptive flush interval tuning.
    adaptive: bool,
}

impl InputBuffer {
    pub fn new(flush_interval: Duration, max_size: usize) -> Self {
        Self {
            data: Vec::new(),
            deadline: Instant::now() + Duration::from_secs(3600),
            flush_interval,
            max_size,
            utf8_carry: Vec::new(),
            passthrough: false,
            binary_mode: false,
            adaptive: false,
        }
    }

    /// Returns `true` if `byte` should be flushed immediately.
    ///
    /// Enter/CR, Ctrl+C/D/Z: command boundaries.
    /// Backspace (DEL, 0x7F) and Tab: must not be held — user feels these immediately.
    pub fn is_immediate(byte: u8) -> bool {
        matches!(byte, b'\n' | b'\r' | 0x03 | 0x04 | 0x1A | 0x7F | b'\t')
    }

    /// Push a byte; in passthrough/binary mode goes directly to `data`, otherwise
    /// drain complete UTF-8 sequences into `data` via carry buffer.
    pub fn push(&mut self, byte: u8) {
        if self.passthrough || self.binary_mode {
            let was_empty = self.data.is_empty();
            self.data.push(byte);
            if was_empty {
                self.deadline = Instant::now() + self.flush_interval;
            }
            return;
        }

        let was_empty = self.data.is_empty() && self.utf8_carry.is_empty();
        self.utf8_carry.push(byte);

        match std::str::from_utf8(&self.utf8_carry) {
            Ok(_) => {
                self.data.extend_from_slice(&self.utf8_carry);
                self.utf8_carry.clear();
            }
            Err(e) => {
                let valid_up_to = e.valid_up_to();
                if valid_up_to > 0 {
                    let remainder = self.utf8_carry[valid_up_to..].to_vec();
                    self.data.extend_from_slice(&self.utf8_carry[..valid_up_to]);
                    self.utf8_carry = remainder;
                }
                if e.error_len().is_some() {
                    self.data.extend_from_slice(&self.utf8_carry);
                    self.utf8_carry.clear();
                }
            }
        }

        if was_empty && !self.data.is_empty() {
            self.deadline = Instant::now() + self.flush_interval;
        }
    }

    /// Push byte and return `true` if caller should flush now.
    pub fn push_and_maybe_flush(&mut self, byte: u8) -> bool {
        self.push(byte);
        Self::is_immediate(byte) || self.should_flush()
    }

    /// True if buffer should be flushed: passthrough always, or deadline/max_size reached.
    pub fn should_flush(&self) -> bool {
        if self.data.is_empty() {
            return false;
        }
        if self.passthrough {
            return true;
        }
        self.data.len() >= self.max_size || Instant::now() >= self.deadline
    }

    /// Drain and return all buffered bytes; resets deadline.
    pub fn take(&mut self) -> Vec<u8> {
        let out = std::mem::take(&mut self.data);
        self.deadline = Instant::now() + Duration::from_secs(3600);
        out
    }

    /// Switch to passthrough mode: every push is immediately available via take().
    pub fn set_passthrough(&mut self, enabled: bool) {
        self.passthrough = enabled;
    }

    /// Switch to binary mode: UTF-8 boundary checking disabled.
    pub fn set_binary_mode(&mut self, enabled: bool) {
        self.binary_mode = enabled;
    }

    /// Enable or disable RTT-based adaptive interval tuning.
    pub fn set_adaptive(&mut self, enabled: bool) {
        self.adaptive = enabled;
    }

    /// Returns true if buffer has reached max_size (caller should apply backpressure).
    pub fn is_full(&self) -> bool {
        self.data.len() >= self.max_size
    }

    /// Adjust flush interval based on observed RTT.
    /// RTT < 50ms  → interval = max(5ms, rtt / 2)
    /// RTT 50-200ms → interval = 20ms (sweet spot, no change)
    /// RTT > 200ms → interval = min(100ms, rtt * 3/10)
    pub fn set_adaptive_interval(&mut self, rtt: Duration) {
        if !self.adaptive {
            return;
        }
        self.flush_interval = calculate_adaptive_interval(rtt);
    }

    pub fn flush_interval(&self) -> Duration {
        self.flush_interval
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn deadline(&self) -> Instant {
        self.deadline
    }

    pub fn has_incomplete_utf8(&self) -> bool {
        !self.utf8_carry.is_empty()
    }

    pub fn is_passthrough(&self) -> bool {
        self.passthrough
    }
}

fn calculate_adaptive_interval(rtt: Duration) -> Duration {
    const MIN: Duration = Duration::from_millis(5);
    const MAX: Duration = Duration::from_millis(100);
    const THRESHOLD_LOW: Duration = Duration::from_millis(50);
    const THRESHOLD_HIGH: Duration = Duration::from_millis(200);

    if rtt < THRESHOLD_LOW {
        (rtt / 2).max(MIN)
    } else if rtt > THRESHOLD_HIGH {
        (rtt * 3 / 10).min(MAX)
    } else {
        Duration::from_millis(20)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::time::Duration;

    fn make_buffer() -> InputBuffer {
        InputBuffer::new(Duration::from_millis(20), 512)
    }

    // --- Phase 1 tests (unchanged) ---

    #[test]
    fn empty_buffer_does_not_flush() {
        let buf = make_buffer();
        assert!(!buf.should_flush());
    }

    #[test]
    fn single_byte_arms_deadline() {
        let mut buf = make_buffer();
        buf.push(b'a');
        assert!(!buf.should_flush());
    }

    #[test]
    fn deadline_expired_triggers_flush() {
        let mut buf = InputBuffer::new(Duration::from_millis(0), 512);
        buf.push(b'a');
        assert!(buf.should_flush());
    }

    #[test]
    fn max_size_triggers_flush() {
        let mut buf = InputBuffer::new(Duration::from_millis(20), 3);
        buf.push(b'a');
        buf.push(b'b');
        assert!(!buf.should_flush());
        buf.push(b'c');
        assert!(buf.should_flush());
    }

    #[test]
    fn take_clears_buffer_and_returns_bytes() {
        let mut buf = make_buffer();
        buf.push(b'x');
        buf.push(b'y');
        let out = buf.take();
        assert_eq!(out, b"xy");
        assert!(buf.is_empty());
    }

    #[test]
    fn take_on_empty_returns_empty_vec() {
        let mut buf = make_buffer();
        assert_eq!(buf.take(), b"");
    }

    #[test]
    fn is_empty_true_initially() {
        let buf = make_buffer();
        assert!(buf.is_empty());
    }

    #[test]
    fn is_empty_false_after_push() {
        let mut buf = make_buffer();
        buf.push(b'a');
        assert!(!buf.is_empty());
    }

    #[test]
    fn len_tracks_data_bytes() {
        let mut buf = make_buffer();
        assert_eq!(buf.len(), 0);
        buf.push(b'a');
        assert_eq!(buf.len(), 1);
        buf.push(b'b');
        assert_eq!(buf.len(), 2);
    }

    #[test]
    fn enter_lf_is_immediate() {
        assert!(InputBuffer::is_immediate(b'\n'));
    }

    #[test]
    fn enter_cr_is_immediate() {
        assert!(InputBuffer::is_immediate(b'\r'));
    }

    #[test]
    fn ctrl_c_is_immediate() {
        assert!(InputBuffer::is_immediate(0x03));
    }

    #[test]
    fn ctrl_d_is_immediate() {
        assert!(InputBuffer::is_immediate(0x04));
    }

    #[test]
    fn ctrl_z_is_immediate() {
        assert!(InputBuffer::is_immediate(0x1A));
    }

    #[test]
    fn backspace_is_immediate() {
        assert!(InputBuffer::is_immediate(0x7F));
    }

    #[test]
    fn tab_is_immediate() {
        assert!(InputBuffer::is_immediate(b'\t'));
    }

    #[test]
    fn regular_char_not_immediate() {
        assert!(!InputBuffer::is_immediate(b'a'));
    }

    #[test]
    fn nul_byte_not_immediate() {
        assert!(!InputBuffer::is_immediate(0x00));
    }

    #[test]
    fn push_and_maybe_flush_returns_true_on_enter() {
        let mut buf = make_buffer();
        assert!(buf.push_and_maybe_flush(b'\n'));
    }

    #[test]
    fn push_and_maybe_flush_returns_false_on_regular_char() {
        let mut buf = make_buffer();
        assert!(!buf.push_and_maybe_flush(b'a'));
    }

    #[test]
    fn push_and_maybe_flush_accumulates_before_enter() {
        let mut buf = make_buffer();
        buf.push_and_maybe_flush(b'l');
        buf.push_and_maybe_flush(b's');
        let flush = buf.push_and_maybe_flush(b'\n');
        assert!(flush);
        assert_eq!(buf.take(), b"ls\n");
    }

    #[test]
    fn utf8_complete_two_byte_char_passes_through() {
        let mut buf = make_buffer();
        buf.push(0xC3);
        buf.push(0xA9); // 'é'
        assert_eq!(buf.len(), 2);
        assert!(!buf.has_incomplete_utf8());
    }

    #[test]
    fn utf8_incomplete_first_byte_held_back() {
        let mut buf = make_buffer();
        buf.push(0xC3); // first byte of 'é', incomplete
        assert!(buf.has_incomplete_utf8());
        assert_eq!(buf.len(), 0);
    }

    #[test]
    fn utf8_completed_by_second_byte() {
        let mut buf = make_buffer();
        buf.push(0xC3);
        assert!(buf.has_incomplete_utf8());
        buf.push(0xA9);
        assert!(!buf.has_incomplete_utf8());
        assert_eq!(buf.len(), 2);
    }

    #[test]
    fn utf8_three_byte_sequence_held_until_complete() {
        // '€' = 0xE2 0x82 0xAC
        let mut buf = make_buffer();
        buf.push(0xE2);
        assert!(buf.has_incomplete_utf8());
        buf.push(0x82);
        assert!(buf.has_incomplete_utf8());
        buf.push(0xAC);
        assert!(!buf.has_incomplete_utf8());
        assert_eq!(buf.len(), 3);
    }

    // --- Phase 2 tests ---

    #[test]
    fn passthrough_mode_push_goes_directly_to_ready() {
        let mut buf = make_buffer();
        buf.set_passthrough(true);
        buf.push(b'a');
        assert_eq!(buf.len(), 1);
        assert_eq!(buf.take(), b"a");
    }

    #[test]
    fn passthrough_mode_skips_deadline() {
        let mut buf = InputBuffer::new(Duration::from_secs(100), 512);
        buf.set_passthrough(true);
        buf.push(b'a');
        // should_flush ignores deadline in passthrough mode
        assert!(buf.should_flush());
    }

    #[test]
    fn binary_mode_skips_utf8_check() {
        let mut buf = make_buffer();
        buf.set_binary_mode(true);
        buf.push(0xFF);
        buf.push(0xFE);
        assert_eq!(buf.len(), 2);
        assert!(!buf.has_incomplete_utf8());
        assert_eq!(buf.take(), &[0xFF, 0xFE]);
    }

    #[test]
    fn is_full_false_below_max_size() {
        let mut buf = InputBuffer::new(Duration::from_millis(20), 3);
        buf.push(b'a');
        buf.push(b'b');
        assert!(!buf.is_full());
    }

    #[test]
    fn is_full_true_at_max_size() {
        let mut buf = InputBuffer::new(Duration::from_millis(20), 3);
        buf.push(b'a');
        buf.push(b'b');
        buf.push(b'c');
        assert!(buf.is_full());
    }

    #[test]
    fn adaptive_interval_clamps_to_minimum() {
        // rtt=1ms < 50ms: interval = max(5ms, 1ms/2) = 5ms
        let mut buf = make_buffer();
        buf.set_adaptive(true);
        buf.set_adaptive_interval(Duration::from_millis(1));
        assert_eq!(buf.flush_interval(), Duration::from_millis(5));
    }

    #[test]
    fn adaptive_interval_clamps_to_maximum() {
        // rtt=1000ms > 200ms: interval = min(100ms, 1000ms*3/10) = 100ms
        let mut buf = make_buffer();
        buf.set_adaptive(true);
        buf.set_adaptive_interval(Duration::from_millis(1000));
        assert_eq!(buf.flush_interval(), Duration::from_millis(100));
    }

    #[test]
    fn adaptive_interval_scales_with_rtt() {
        let mut buf = make_buffer();
        buf.set_adaptive(true);

        // In the 50-200ms sweet spot, interval stays at 20ms
        buf.set_adaptive_interval(Duration::from_millis(100));
        assert_eq!(buf.flush_interval(), Duration::from_millis(20));

        // Above 200ms, interval scales: 300ms * 3/10 = 90ms
        buf.set_adaptive_interval(Duration::from_millis(300));
        assert_eq!(buf.flush_interval(), Duration::from_millis(90));
    }

    #[test]
    fn adaptive_interval_no_change_when_adaptive_disabled() {
        let mut buf = InputBuffer::new(Duration::from_millis(20), 512);
        // adaptive not enabled
        buf.set_adaptive_interval(Duration::from_millis(1000));
        assert_eq!(buf.flush_interval(), Duration::from_millis(20));
    }

    // --- Property tests ---

    proptest! {
        #[test]
        fn prop_flush_never_splits_utf8(input in ".*") {
            let bytes = input.as_bytes();
            let mut buf = InputBuffer::new(Duration::from_millis(20), 64);
            let mut output: Vec<u8> = Vec::new();

            for &byte in bytes {
                buf.push(byte);
                if buf.is_full() {
                    output.extend(buf.take());
                }
            }
            output.extend(buf.take());

            // Every byte of valid UTF-8 input must come back and the
            // reassembled output must be valid UTF-8.
            prop_assert_eq!(&output, bytes);
            prop_assert!(std::str::from_utf8(&output).is_ok());
        }

        #[test]
        fn prop_immediate_bytes_never_delayed(
            b in any::<u8>().prop_filter("must be immediate", |&b| InputBuffer::is_immediate(b))
        ) {
            let mut buf = InputBuffer::new(Duration::from_millis(500), 512);
            let should_flush = buf.push_and_maybe_flush(b);
            prop_assert!(should_flush, "immediate byte 0x{:02X} should trigger flush", b);
        }

        #[test]
        fn prop_take_returns_all_pushed_non_carry_bytes(
            input in proptest::collection::vec(0u8..=127u8, 0..256)
        ) {
            // ASCII only — no multi-byte carry possible
            let mut buf = InputBuffer::new(Duration::from_millis(20), 1024);
            for &byte in &input {
                buf.push(byte);
            }
            let output = buf.take();
            prop_assert_eq!(output, input);
        }
    }
}
