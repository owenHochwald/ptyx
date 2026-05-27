# Phase 2 — Buffering Excellence + Metrics

**Goal:** Make the buffer smart — adaptive timing, binary/raw passthrough, backpressure, session metrics, and a live stats overlay.

**Blocked on:** Phase 1 acceptance criteria must be fully green.  
**Estimated effort:** 1–2 days

---

## Why This Phase Matters More Than Echo Prediction

Echo prediction is flashy but fragile. A misprediction requires display correction; getting that wrong corrupts the user's terminal. Buffering, by contrast, is the core latency win:

- On a 100ms RTT link, batching 3 keystrokes into one flush saves 2 round-trips — 200ms of perceived latency — with **zero risk of corrupting output**.
- Adaptive flush timing means the buffer tunes itself: short window for low-latency links, longer for satellite/VPN.
- Binary passthrough means `ptyx` doesn't break `scp`, `rsync`, or `vim`'s raw mode.

Phase 2 makes buffering production-quality. Echo prediction (Phase 3) is an enhancement on top of an already-good proxy.

---

## New Files in This Phase

| File | Responsibility | Hard limit |
|------|----------------|------------|
| `src/metrics.rs` | `SessionMetrics` RTT ring + bytes-saved | 150 lines |

### Modified Files

| File | What changes |
|------|--------------|
| `src/buffer.rs` | Adaptive interval, passthrough mode, binary mode, `is_full()` |
| `src/proxy.rs` | Wire metrics, backpressure, stats overlay, new CLI flags |
| `src/config.rs` | New CLI flags: `--stats`, `--buffer`, `--max-size`, `--no-buffer` |
| `src/lib.rs` | `pub mod metrics;` |
| `CLAUDE.md` | Add `metrics.rs` row to module table |
| `docs/INDEX.md` | (already has metrics.rs placeholder) |

---

## TDD Order

### Step 1 — metrics.rs tests

All pure math — no PTY, no async. Fastest feedback.

```rust
// src/metrics.rs #[cfg(test)]

fn rtt_estimate_averages_samples()
// Record 100ms, 200ms, 300ms → estimate = 200ms

fn rtt_estimate_on_empty_returns_zero()

fn rtt_ring_buffer_evicts_oldest()
// capacity=3; push 4 samples; oldest evicted from average

fn record_flush_accumulates_bytes_saved()
// batch_size=3 means 3 keystrokes sent as one; bytes_saved += 2

fn bytes_saved_zero_when_all_flushed_singly()
// Every flush is batch_size=1 → bytes_saved stays 0

fn prediction_accuracy_vacuously_perfect_when_empty()
// No data → 1.0 (reserved slot; not used until Phase 3)

fn buffer_depth_tracks_pending()
fn buffer_depth_zero_after_flush()
```

### Step 2 — buffer.rs additions

```rust
// src/buffer.rs #[cfg(test)] additions

fn passthrough_mode_push_goes_directly_to_ready()
// set_passthrough(true); push b'a'; take() = [b'a'] immediately

fn passthrough_mode_skips_deadline()
// In passthrough mode, should_flush() = true for any non-empty buffer

fn binary_mode_skips_utf8_check()
// set_binary_mode(true); push 0xFF 0xFE; take() returns [0xFF, 0xFE] unmodified

fn is_full_false_below_max_size()
fn is_full_true_at_max_size()

fn adaptive_interval_clamps_to_minimum()
// rtt=1ms → interval set to floor (e.g. 5ms minimum)

fn adaptive_interval_clamps_to_maximum()
// rtt=1000ms → interval set to ceiling (e.g. 100ms maximum)

fn adaptive_interval_scales_linearly_in_range()
// rtt=100ms → interval ~20ms; rtt=300ms → interval ~40ms

// proptest suite
proptest! {
    fn prop_flush_never_splits_utf8(input in proptest::collection::vec(0u8..=0xFF, 0..256)) {
        // Push random bytes; every chunk returned by take() is valid UTF-8
        // OR was flagged as binary
    }

    fn prop_immediate_bytes_never_delayed(b in proptest::sample::select(IMMEDIATE_BYTES)) {
        // push_and_maybe_flush(b) always returns true for immediate bytes
    }

    fn prop_take_returns_all_pushed_non_carry_bytes(input in ...) {
        // Sum of all take() results = all pushed bytes minus any outstanding utf8_carry
    }
}
```

### Step 3 — config.rs additions

```rust
fn stats_flag_parsed_from_cli()
fn buffer_interval_override_from_cli()
fn max_size_override_from_cli()
fn no_buffer_flag_sets_passthrough()
fn verbose_flag_sets_tracing_level()
```

### Step 4 — integration tests

```rust
// tests/integration/buffer_advanced.rs

fn raw_mode_output_triggers_passthrough()
// Open PTY pair. Write "\x1b[?1049h" (enter alt screen) to master output side.
// Check that proxy switches buffer to passthrough mode.

fn backpressure_pauses_stdin_reads()
// Fill buffer to max_size. Verify proxy stops reading stdin until flush.
// (Hint: use a channel and confirm no new receives arrive until flush completes)

fn adaptive_interval_applied_after_rtt_sample()
// After a flush+response cycle with known simulated RTT, check that
// buffer.flush_interval changed accordingly.

// tests/integration/metrics.rs
fn metrics_bytes_saved_nonzero_after_batched_flush()
fn metrics_rtt_nonzero_after_roundtrip()
```

---

## Implementation Details

### src/metrics.rs

```rust
#[derive(Debug)]
pub struct SessionMetrics {
    // Ring buffer of RTT samples (most recent N)
    rtt_samples: VecDeque<Duration>,
    rtt_capacity: usize,
    
    // Bytes tracking
    total_bytes_sent: u64,
    total_flushes: u64,
    bytes_saved: u64,         // (total_bytes_sent - total_flushes) if all sent singly
    
    // Prediction (Phase 3 placeholder — unused until then)
    prediction_hits: u64,
    prediction_misses: u64,
}

impl SessionMetrics {
    pub fn new(rtt_capacity: usize) -> Self { ... }
    
    /// Call after every PTY round-trip with the elapsed time.
    pub fn record_rtt(&mut self, rtt: Duration) { ... }
    
    /// Call after every buffer flush.
    /// `batch_size` = number of bytes in the flush; used to compute bytes saved.
    pub fn record_flush(&mut self, batch_size: usize) {
        self.total_bytes_sent += batch_size as u64;
        self.total_flushes += 1;
        if batch_size > 1 {
            self.bytes_saved += (batch_size as u64) - 1;
        }
    }
    
    pub fn rtt_estimate(&self) -> Duration {
        if self.rtt_samples.is_empty() { return Duration::ZERO; }
        let sum: Duration = self.rtt_samples.iter().sum();
        sum / self.rtt_samples.len() as u32
    }
    
    pub fn bytes_saved(&self) -> u64 { self.bytes_saved }
    pub fn total_flushes(&self) -> u64 { self.total_flushes }
    
    pub fn prediction_accuracy(&self) -> f64 {
        let total = self.prediction_hits + self.prediction_misses;
        if total == 0 { 1.0 } else { self.prediction_hits as f64 / total as f64 }
    }
}
```

### src/buffer.rs additions

New fields on `InputBuffer`:
```rust
pub struct InputBuffer {
    // ... existing fields ...
    passthrough: bool,           // raw mode active — flush immediately
    binary_mode: bool,           // binary protocol — skip UTF-8 check
    adaptive: bool,              // enable RTT-based interval tuning
}
```

New methods:
```rust
impl InputBuffer {
    /// Switch to passthrough: every push() is immediately available via take().
    pub fn set_passthrough(&mut self, enabled: bool) { self.passthrough = enabled; }
    
    /// Switch to binary mode: UTF-8 boundary checking disabled.
    pub fn set_binary_mode(&mut self, enabled: bool) { self.binary_mode = enabled; }
    
    /// Returns true if buffer has reached max_size (caller should apply backpressure).
    pub fn is_full(&self) -> bool { self.data.len() >= self.max_size }
    
    /// Adjust flush interval based on observed RTT.
    /// RTT < 50ms  → interval = max(5ms, rtt * 0.5)
    /// RTT 50-200ms→ interval = 20ms (default, no change)
    /// RTT > 200ms → interval = min(100ms, rtt * 0.3)
    pub fn set_adaptive_interval(&mut self, rtt: Duration) {
        if !self.adaptive { return; }
        let new_interval = calculate_adaptive_interval(rtt);
        self.flush_interval = new_interval;
    }
}

fn calculate_adaptive_interval(rtt: Duration) -> Duration {
    const MIN: Duration = Duration::from_millis(5);
    const MAX: Duration = Duration::from_millis(100);
    const DEFAULT_THRESHOLD_LOW: Duration = Duration::from_millis(50);
    const DEFAULT_THRESHOLD_HIGH: Duration = Duration::from_millis(200);
    
    if rtt < DEFAULT_THRESHOLD_LOW {
        (rtt / 2).max(MIN)
    } else if rtt > DEFAULT_THRESHOLD_HIGH {
        (rtt * 3 / 10).min(MAX)
    } else {
        Duration::from_millis(20)  // sweet spot — keep default
    }
}
```

### Passthrough trigger in proxy.rs

When the proxy reads output containing alt-screen or raw-mode escape sequences, it switches the buffer to passthrough:

```rust
// In the PTY output handler branch of select!
Ok(n) => {
    let chunk = &output_buf[..n];
    
    // Detect raw mode entry/exit
    if contains_enter_raw(chunk) {
        self.buffer.set_passthrough(true);
        tracing::debug!("raw mode detected, buffer → passthrough");
    } else if contains_exit_raw(chunk) {
        self.buffer.set_passthrough(false);
        tracing::debug!("raw mode exit, buffer → normal");
    }
    
    stdout.write_all(chunk).await?;
    stdout.flush().await?;
    
    // Record RTT: time from last flush to first response byte
    if let Some(flush_time) = self.last_flush_at.take() {
        self.metrics.record_rtt(flush_time.elapsed());
        self.buffer.set_adaptive_interval(self.metrics.rtt_estimate());
    }
}

fn contains_enter_raw(bytes: &[u8]) -> bool {
    const ENTER: &[u8] = b"\x1b[?1049h";
    bytes.windows(ENTER.len()).any(|w| w == ENTER)
}

fn contains_exit_raw(bytes: &[u8]) -> bool {
    const EXIT: &[u8] = b"\x1b[?1049l";
    bytes.windows(EXIT.len()).any(|w| w == EXIT)
}
```

### Backpressure in proxy.rs

Stop reading stdin when buffer is full:

```rust
// In the stdin read branch:
// Guard: only select this branch when buffer is not full
n = stdin.read(&mut input_byte), if !self.buffer.is_full() => {
    ...
}
```

tokio's `select!` guard (`if <condition>`) is the cleanest backpressure mechanism — no extra channels or flags needed.

### Stats overlay

When `--stats` flag is set, render a one-line status bar at the bottom of the terminal using crossterm. Update at ~4Hz (250ms timer in select!):

```
[ptyx] RTT: 142ms  saved: 1.2KB  flushes: 47  buf: 0/512
```

```rust
// In select! — new branch:
_ = tokio::time::sleep(Duration::from_millis(250)), if self.config.show_stats => {
    render_stats_bar(&self.metrics)?;
}

fn render_stats_bar(m: &SessionMetrics) -> anyhow::Result<()> {
    use crossterm::{cursor, terminal, style, execute};
    let (cols, rows) = terminal::size()?;
    let bar = format!(
        "[ptyx] RTT: {}ms  saved: {}B  flushes: {}",
        m.rtt_estimate().as_millis(),
        m.bytes_saved(),
        m.total_flushes(),
    );
    execute!(
        std::io::stdout(),
        cursor::SavePosition,
        cursor::MoveTo(0, rows - 1),
        terminal::Clear(terminal::ClearType::CurrentLine),
        style::Print(&bar[..bar.len().min(cols as usize)]),
        cursor::RestorePosition,
    )?;
    Ok(())
}
```

---

## New CLI Flags (src/config.rs additions)

```
ptyx [OPTIONS] <user@host> [-- <ssh_args>...]

Options:
  -b, --buffer <ms>       Flush interval in milliseconds [default: 20]
  -s, --max-size <bytes>  Max buffer size before forced flush [default: 512]
      --no-buffer         Disable buffering (passthrough mode)
      --stats             Show live metrics bar at bottom of terminal
      --adaptive          Enable RTT-based adaptive flush interval
  -v, --verbose           Enable debug logging (RUST_LOG=ptyx=debug)
  -h, --help              Print help
  -V, --version           Print version
```

---

## Benchmarks (compare vs Phase 1 baseline)

```bash
# Must run before and after Phase 2 changes:
cargo bench -- --baseline phase1
```

New benchmarks to add in `benches/buffer.rs`:
- `bench_passthrough_overhead` — passthrough mode push+take vs normal path
- `bench_adaptive_interval_update` — `set_adaptive_interval()` call < 200ns
- `bench_contains_enter_raw` — sliding window scan < 100ns for 1KB chunk
- `bench_metrics_record_flush` — < 50ns per record call

Target: no benchmark regresses > 10% vs phase1 baseline.

---

## Acceptance Criteria

```bash
cargo test                    # ✓ all unit tests green
cargo test --test '*'         # ✓ all integration tests green
cargo clippy -- -D warnings   # ✓ zero warnings
cargo fmt --check             # ✓ clean
cargo bench -- --baseline phase1  # ✓ no regressions
```

Manual:
- [ ] `ptyx --stats user@host` — live RTT + bytes-saved bar visible, updates in real-time
- [ ] `ptyx --no-buffer user@host` — works; buffering bypassed (use `strace` to confirm single-byte writes)
- [ ] `ptyx user@host` then open `vim` — buffer switches to passthrough, no input delay or corruption
- [ ] `ptyx user@host` then close `vim` — buffer returns to normal mode, batching resumes
- [ ] Buffer fills to max_size while SSH is slow — no new stdin reads until flushed (verify via debug log)
- [ ] On a simulated high-RTT link (tc netem): flush interval adapts upward over ~10s
- [ ] No benchmark regression > 10% vs phase1 baseline

---

## What Phase 2 Does NOT Include

- ❌ Echo prediction / local display (Phase 3)
- ❌ Session recording / replay (Phase 4)
- ❌ TOML config file (Phase 4)
- ❌ Plugin system (Phase 4+)
- ❌ Reconnect on network drop (Phase 5+)
