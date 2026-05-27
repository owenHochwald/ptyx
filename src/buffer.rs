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
}

impl InputBuffer {
    pub fn new(flush_interval: Duration, max_size: usize) -> Self {
        Self {
            data: Vec::new(),
            deadline: Instant::now() + Duration::from_secs(3600),
            flush_interval,
            max_size,
            utf8_carry: Vec::new(),
        }
    }

    /// Returns `true` if `byte` should be flushed immediately.
    ///
    /// Enter/CR, Ctrl+C/D/Z: command boundaries.
    /// Backspace (DEL, 0x7F) and Tab: must not be held — user feels these immediately.
    pub fn is_immediate(byte: u8) -> bool {
        matches!(byte, b'\n' | b'\r' | 0x03 | 0x04 | 0x1A | 0x7F | b'\t')
    }

    /// Push a byte into the carry buffer, drain complete UTF-8 sequences into `data`.
    pub fn push(&mut self, byte: u8) {
        let was_empty = self.data.is_empty() && self.utf8_carry.is_empty();
        self.utf8_carry.push(byte);

        match std::str::from_utf8(&self.utf8_carry) {
            Ok(_) => {
                // Complete valid UTF-8 — move everything into data
                self.data.extend_from_slice(&self.utf8_carry);
                self.utf8_carry.clear();
            }
            Err(e) => {
                // Check if the error is recoverable (incomplete sequence at end)
                let valid_up_to = e.valid_up_to();
                if valid_up_to > 0 {
                    // Move the valid prefix into data, keep remainder in carry
                    let remainder = self.utf8_carry[valid_up_to..].to_vec();
                    self.data.extend_from_slice(&self.utf8_carry[..valid_up_to]);
                    self.utf8_carry = remainder;
                }
                // If error_len is None, it's an incomplete sequence at end — keep in carry
                // If error_len is Some, it's an invalid byte — treat as binary
                if e.error_len().is_some() {
                    // Irrecoverably invalid — flush carry as binary
                    self.data.extend_from_slice(&self.utf8_carry);
                    self.utf8_carry.clear();
                }
                // else: incomplete sequence, keep in utf8_carry
            }
        }

        // Arm deadline only when data transitions from empty to non-empty
        if was_empty && !self.data.is_empty() {
            self.deadline = Instant::now() + self.flush_interval;
        }
    }

    /// Push byte and return `true` if caller should flush now.
    pub fn push_and_maybe_flush(&mut self, byte: u8) -> bool {
        self.push(byte);
        Self::is_immediate(byte) || self.should_flush()
    }

    /// True if deadline has passed or max_size reached.
    pub fn should_flush(&self) -> bool {
        if self.data.is_empty() {
            return false;
        }
        self.data.len() >= self.max_size || Instant::now() >= self.deadline
    }

    /// Drain and return all buffered bytes; resets deadline.
    pub fn take(&mut self) -> Vec<u8> {
        let out = std::mem::take(&mut self.data);
        self.deadline = Instant::now() + Duration::from_secs(3600);
        out
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn make_buffer() -> InputBuffer {
        InputBuffer::new(Duration::from_millis(20), 512)
    }

    // Group 1: flush conditions
    #[test]
    fn empty_buffer_does_not_flush() {
        let buf = make_buffer();
        assert!(!buf.should_flush());
    }

    #[test]
    fn single_byte_arms_deadline() {
        let mut buf = make_buffer();
        buf.push(b'a');
        // deadline was in the far future before; after push it's ~20ms from now
        // It should NOT be expired yet
        assert!(!buf.should_flush());
    }

    #[test]
    fn deadline_expired_triggers_flush() {
        let mut buf = InputBuffer::new(Duration::from_millis(0), 512);
        buf.push(b'a');
        // 0ms interval means deadline is immediately in the past
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

    // Group 2: take semantics
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

    // Group 3: is_immediate
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

    // Group 4: push_and_maybe_flush
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

    // Group 5: UTF-8 safety
    #[test]
    fn utf8_complete_two_byte_char_passes_through() {
        // 'é' = 0xC3 0xA9
        let mut buf = make_buffer();
        buf.push(0xC3);
        buf.push(0xA9);
        assert_eq!(buf.len(), 2);
        assert!(!buf.has_incomplete_utf8());
    }

    #[test]
    fn utf8_incomplete_first_byte_held_back() {
        let mut buf = make_buffer();
        buf.push(0xC3); // first byte of 'é', incomplete
        assert!(buf.has_incomplete_utf8());
        assert_eq!(buf.len(), 0); // not in data yet
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
}
