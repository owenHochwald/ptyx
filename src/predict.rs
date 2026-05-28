use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Predicts the local echo for typed input in cooked mode, enabling zero-RTT display.
#[derive(Debug)]
pub struct EchoPredictor {
    pending: VecDeque<PendingInput>,
    miss_streak: usize,
    miss_threshold: usize,
    pub enabled: bool,
    next_id: u64,
}

#[derive(Debug)]
struct PendingInput {
    sent_at: Instant,
    predicted: String,
    #[allow(dead_code)]
    bytes: Vec<u8>,
}

/// Result of reconciling a server echo against a predicted echo.
#[derive(Debug)]
pub enum ReconcileResult {
    /// Server echo matched prediction exactly; RTT is the round-trip time.
    Confirmed { rtt: Duration },
    /// Server echo differed from prediction; `correction` holds the actual output.
    Mispredicted { rtt: Duration, correction: String },
    /// No pending prediction — server output is unrelated to typed input.
    Passthrough,
}

impl EchoPredictor {
    /// Create a new predictor that disables itself after `miss_threshold` consecutive misses.
    pub fn new(miss_threshold: usize) -> Self {
        Self {
            pending: VecDeque::new(),
            miss_streak: 0,
            miss_threshold,
            enabled: true,
            next_id: 0,
        }
    }

    /// Predict the terminal echo for `input` bytes in cooked mode.
    ///
    /// Returns `None` when prediction is disabled. Returns `Some("")` for inputs
    /// that produce no visible echo (control characters).
    pub fn predict(&mut self, input: &[u8]) -> Option<String> {
        if !self.enabled {
            return None;
        }

        let mut out = String::new();
        for &byte in input {
            match byte {
                // Printable ASCII echoed as-is
                0x20..=0x7E => out.push(byte as char),
                // Enter: shell echoes \r\n
                b'\r' | b'\n' => out.push_str("\r\n"),
                // Backspace/DEL: shell echoes erase sequence
                0x08 | 0x7F => out.push_str("\x08 \x08"),
                // Tab: echo as-is (completion makes this imprecise, but good enough)
                b'\t' => out.push('\t'),
                // Control chars don't echo
                _ => {}
            }
        }

        self.next_id += 1;
        self.pending.push_back(PendingInput {
            sent_at: Instant::now(),
            predicted: out.clone(),
            bytes: input.to_vec(),
        });

        Some(out)
    }

    /// Reconcile actual server output against the oldest pending prediction.
    pub fn reconcile(&mut self, actual: &[u8]) -> ReconcileResult {
        let Some(pending) = self.pending.pop_front() else {
            return ReconcileResult::Passthrough;
        };

        let rtt = pending.sent_at.elapsed();
        let actual_str = String::from_utf8_lossy(actual);

        if actual_str == pending.predicted {
            self.miss_streak = 0;
            ReconcileResult::Confirmed { rtt }
        } else {
            self.miss_streak += 1;
            if self.miss_streak >= self.miss_threshold {
                self.enabled = false;
                tracing::warn!(miss_streak = self.miss_streak, "echo prediction disabled");
            }
            ReconcileResult::Mispredicted {
                rtt,
                correction: actual_str.into_owned(),
            }
        }
    }

    /// Inspect server output for raw-mode entry/exit sequences; disables or re-enables
    /// prediction accordingly, and clears stale pending entries on raw-mode entry.
    pub fn check_output_for_raw_mode(&mut self, output: &[u8]) {
        const ENTER_RAW: &[u8] = b"\x1b[?1049h";
        const EXIT_RAW: &[u8] = b"\x1b[?1049l";

        if output.windows(ENTER_RAW.len()).any(|w| w == ENTER_RAW) {
            self.enabled = false;
            self.pending.clear();
            tracing::debug!("raw mode detected, disabling prediction");
            return;
        }
        if output.windows(EXIT_RAW.len()).any(|w| w == EXIT_RAW) {
            self.enabled = true;
            tracing::debug!("raw mode exit, re-enabling prediction");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_predictor() -> EchoPredictor {
        EchoPredictor::new(3)
    }

    #[test]
    fn predicts_printable_ascii() {
        let mut p = make_predictor();
        let predicted = p.predict(b"hello").unwrap();
        assert_eq!(predicted, "hello");
    }

    #[test]
    fn predicts_backspace_as_erase_sequence() {
        let mut p = make_predictor();
        let predicted = p.predict(&[0x7F]).unwrap(); // DEL
        assert_eq!(predicted, "\x08 \x08");
    }

    #[test]
    fn control_chars_not_echoed() {
        let mut p = make_predictor();
        let predicted = p.predict(&[0x01]).unwrap(); // Ctrl+A
        assert_eq!(predicted, "");
    }

    #[test]
    fn confirmed_reconcile_resets_miss_streak() {
        let mut p = make_predictor();
        // Introduce a miss first so streak > 0
        p.predict(b"a");
        p.reconcile(b"b"); // mismatch → miss_streak = 1
        assert_eq!(p.miss_streak, 1);

        p.predict(b"c");
        let result = p.reconcile(b"c"); // match → streak resets
        assert!(matches!(result, ReconcileResult::Confirmed { .. }));
        assert_eq!(p.miss_streak, 0);
    }

    #[test]
    fn mispredicted_reconcile_increments_miss_streak() {
        let mut p = make_predictor();
        p.predict(b"a");
        let result = p.reconcile(b"b"); // mismatch
        assert!(matches!(result, ReconcileResult::Mispredicted { .. }));
        assert_eq!(p.miss_streak, 1);
    }

    #[test]
    fn prediction_disabled_after_threshold_misses() {
        let mut p = make_predictor(); // threshold = 3
        for _ in 0..3 {
            p.predict(b"a");
            p.reconcile(b"b"); // mismatch every time
        }
        assert!(!p.enabled);
        assert!(p.predict(b"x").is_none());
    }

    #[test]
    fn raw_mode_escape_disables_prediction() {
        let mut p = make_predictor();
        assert!(p.enabled);
        p.check_output_for_raw_mode(b"\x1b[?1049h"); // enter alt screen
        assert!(!p.enabled);
    }

    #[test]
    fn exit_alt_screen_re_enables_prediction() {
        let mut p = make_predictor();
        p.check_output_for_raw_mode(b"\x1b[?1049h");
        assert!(!p.enabled);
        p.check_output_for_raw_mode(b"\x1b[?1049l"); // exit alt screen
        assert!(p.enabled);
    }

    #[test]
    fn passthrough_result_when_no_pending() {
        let mut p = make_predictor();
        let result = p.reconcile(b"unexpected output");
        assert!(matches!(result, ReconcileResult::Passthrough));
    }

    #[test]
    fn newline_predicts_crlf() {
        let mut p = make_predictor();
        let predicted = p.predict(b"\n").unwrap();
        assert_eq!(predicted, "\r\n");
    }

    #[test]
    fn raw_mode_clears_pending_predictions() {
        let mut p = make_predictor();
        p.predict(b"hello"); // adds a pending entry
        assert!(!p.pending.is_empty());
        p.check_output_for_raw_mode(b"\x1b[?1049h");
        assert!(p.pending.is_empty());
    }
}
