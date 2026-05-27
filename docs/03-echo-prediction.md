# Echo Prediction & Reconciliation

## Why Predict?

In cooked mode, the remote shell echoes every character back. That echo traverses the full RTT before reaching the user's screen. On a 150ms link, typing "hello" means 750ms of blank screen.

ptyx predicts the echo locally and renders it immediately. When the real echo arrives, it reconciles.

## When Prediction Is Valid

| Mode | Predictable? | Reason |
|------|-------------|--------|
| Cooked mode, echo on | ✅ Yes | Kernel echoes exactly what you type (printable chars) |
| Cooked mode, echo off | ❌ No | `stty -echo` — passwords, hidden input |
| Raw mode | ❌ No | Application controls display entirely |
| Binary/protocol | ❌ No | Not a character stream |

Default assumption: **predict**. Disable on first misprediction in raw-mode context.

## EchoPredictor

```rust
pub struct EchoPredictor {
    /// Bytes we've sent but not yet seen confirmed from server
    pending: VecDeque<PendingInput>,
    /// Display buffer of predicted-but-unconfirmed chars
    predicted_display: String,
    /// Accumulated mispredictions — used to auto-disable
    miss_streak: usize,
    /// Disable prediction after N consecutive misses
    miss_threshold: usize,  // default: 3
    pub enabled: bool,
}

struct PendingInput {
    id: u64,
    sent_at: Instant,
    bytes: Vec<u8>,
    predicted: String,
}
```

## Prediction Logic (Cooked Mode)

```rust
impl EchoPredictor {
    pub fn predict(&mut self, input: &[u8]) -> Option<String> {
        if !self.enabled { return None; }

        let mut out = String::new();
        for &byte in input {
            match byte {
                // Printable ASCII — server will echo as-is
                0x20..=0x7E => out.push(byte as char),
                // Enter — server echoes \r\n (shell adds newline)
                b'\r' | b'\n' => out.push_str("\r\n"),
                // Backspace/DEL — server echoes \x08 \x20 \x08 (erase)
                0x08 | 0x7F => out.push_str("\x08 \x08"),
                // Tab — tricky (completion possible); just echo \t
                b'\t' => out.push('\t'),
                // Control chars don't echo
                _ => {}
            }
        }

        let id = self.next_id();
        self.pending.push_back(PendingInput {
            id,
            sent_at: Instant::now(),
            bytes: input.to_vec(),
            predicted: out.clone(),
        });
        self.predicted_display.push_str(&out);
        Some(out)
    }
}
```

## Reconciliation

Called when actual server output arrives on the PTY master fd:

```rust
impl EchoPredictor {
    pub fn reconcile(&mut self, actual: &[u8]) -> ReconcileResult {
        let Some(pending) = self.pending.pop_front() else {
            // Output arrived with nothing pending — passthrough
            return ReconcileResult::Passthrough;
        };

        let rtt = pending.sent_at.elapsed();
        let actual_str = String::from_utf8_lossy(actual);

        if actual_str == pending.predicted {
            // Perfect match — display was already correct
            self.miss_streak = 0;
            ReconcileResult::Confirmed { rtt }
        } else {
            // Mismatch — must correct the display
            self.miss_streak += 1;
            if self.miss_streak >= self.miss_threshold {
                self.enabled = false;
                tracing::warn!("echo prediction disabled after {} misses", self.miss_streak);
            }
            ReconcileResult::Mispredicted {
                rtt,
                correction: actual_str.to_string(),
            }
        }
    }
}

pub enum ReconcileResult {
    Confirmed { rtt: Duration },
    Mispredicted { rtt: Duration, correction: String },
    Passthrough,
}
```

## Display Correction

On misprediction, the proxy must overwrite the predicted text in the user's terminal:

```rust
pub fn apply_correction(correction: &str) {
    // Move cursor back over predicted chars, overwrite, then pad/clear
    // Simplest approach: reprint the whole line
    print!("\r{}\r\n", correction);
    // Sophisticated: use crossterm to erase and redraw inline
}
```

## Misprediction Scenarios

| Scenario | Cause | Detection |
|----------|-------|-----------|
| Remote shell has `stty -echo` | sudo, password prompts | Actual output is empty string |
| Non-ASCII input | UTF-8 multibyte char | Predicted ASCII ≠ actual bytes |
| Shell expansion | `!!`, `!$` | Server echoes expanded form |
| Paste detection | Bracketed paste mode | Server echoes bracket sequences |
| TUI app launched | vim, less, etc. | Server sends escape sequences |
| Tab completion | Shell filled in rest | Output longer than input |

## Auto-Disable Heuristics

```rust
// Detect raw mode: TUI apps send this escape on startup
const ENTER_RAW_INDICATORS: &[&[u8]] = &[
    b"\x1b[?1049h",  // Enter alternate screen (vim, less)
    b"\x1b[?1h",     // Application cursor keys
];

pub fn check_output_for_raw_mode(&mut self, output: &[u8]) {
    for indicator in ENTER_RAW_INDICATORS {
        if output.windows(indicator.len()).any(|w| w == *indicator) {
            self.enabled = false;
            tracing::debug!("raw mode detected, disabling prediction");
            return;
        }
    }
}

// Re-enable when returning to shell (alternate screen exit)
const EXIT_RAW_INDICATORS: &[&[u8]] = &[
    b"\x1b[?1049l",  // Exit alternate screen
];
```

## Sequence Diagram

```
User types 'l'
    │
    ├─► EchoPredictor::predict(b"l") → "l"
    │       display: "l" (instant, t=0ms)
    │
    └─► InputBuffer::push(b'l') → buffering...
            [20ms deadline]
            │
            └─► write "l" to PTY master
                    │
                    [150ms RTT]
                    │
                    server echoes "l"
                    │
                    PTY master readable
                    │
                    └─► EchoPredictor::reconcile("l")
                            → Confirmed { rtt: 150ms }
                            display: already correct, no-op ✓
```
