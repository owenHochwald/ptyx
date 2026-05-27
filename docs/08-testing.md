# Testing Guide

## Philosophy: Tests First

**Write the test before the implementation.** Every module in ptyx must have a failing test written before any code is added. This ensures:
- We know what "done" looks like before we build
- Refactors stay safe
- Benchmarks baseline before optimization

Test file lives next to the module: `src/buffer.rs` → `src/buffer.rs` (inline `#[cfg(test)]`) + `tests/buffer_integration.rs`.

---

## Test Layers

```
tests/
├── unit/           ← inline in src/ via #[cfg(test)]
├── integration/    ← tests/ directory, require a PTY
│   ├── pty_proxy.rs
│   ├── buffering.rs
│   └── echo_predict.rs
└── benches/        ← Criterion benchmarks
    ├── buffer.rs
    └── prediction.rs
```

---

## Unit Tests

### InputBuffer

```rust
// src/buffer.rs — at bottom of file

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn make_buffer() -> InputBuffer {
        InputBuffer::new(Duration::from_millis(20), 512)
    }

    #[test]
    fn empty_buffer_does_not_flush() {
        let buf = make_buffer();
        assert!(!buf.should_flush());
    }

    #[test]
    fn single_byte_arms_deadline() {
        let mut buf = make_buffer();
        buf.push(b'a');
        // Deadline is in the future, not yet
        assert!(!buf.should_flush());
    }

    #[test]
    fn deadline_expired_triggers_flush() {
        let mut buf = InputBuffer::new(Duration::from_millis(0), 512);
        buf.push(b'a');
        // 0ms deadline = already expired
        assert!(buf.should_flush());
    }

    #[test]
    fn max_size_triggers_flush() {
        let mut buf = InputBuffer::new(Duration::from_millis(1000), 3);
        buf.push(b'a');
        buf.push(b'b');
        buf.push(b'c'); // hits max_size=3
        assert!(buf.should_flush());
    }

    #[test]
    fn take_clears_buffer() {
        let mut buf = make_buffer();
        buf.push(b'x');
        buf.push(b'y');
        let taken = buf.take();
        assert_eq!(taken, b"xy");
        assert!(buf.is_empty());
    }

    #[test]
    fn enter_key_is_immediate() {
        assert!(InputBuffer::is_immediate(b'\n'));
        assert!(InputBuffer::is_immediate(b'\r'));
        assert!(InputBuffer::is_immediate(0x03)); // Ctrl+C
        assert!(InputBuffer::is_immediate(0x04)); // Ctrl+D
        assert!(!InputBuffer::is_immediate(b'a'));
    }

    #[test]
    fn push_and_maybe_flush_true_on_enter() {
        let mut buf = make_buffer();
        buf.push(b'l');
        buf.push(b's');
        let flush = buf.push_and_maybe_flush(b'\n');
        assert!(flush);
        assert_eq!(buf.take(), b"ls\n");
    }

    #[test]
    fn utf8_incomplete_sequence_not_flushed_mid_char() {
        let mut buf = make_buffer();
        // First byte of 2-byte UTF-8 sequence (é = 0xC3 0xA9)
        buf.push(0xC3);
        assert!(buf.has_incomplete_utf8());
        // Should not flush incomplete sequence
        assert!(!buf.should_flush_complete());
        buf.push(0xA9);
        assert!(!buf.has_incomplete_utf8());
    }
}
```

### EchoPredictor

```rust
// src/predict.rs

#[cfg(test)]
mod tests {
    use super::*;

    fn make_predictor() -> EchoPredictor {
        EchoPredictor::new(3) // miss_threshold=3
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
        // Ctrl+A is 0x01 — not printable
        let predicted = p.predict(&[0x01]).unwrap();
        assert_eq!(predicted, "");
    }

    #[test]
    fn confirmed_reconcile_increments_hits() {
        let mut p = make_predictor();
        p.predict(b"a");
        let result = p.reconcile(b"a");
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
        let mut p = make_predictor();
        for _ in 0..3 {
            p.predict(b"a");
            p.reconcile(b"b");
        }
        assert!(!p.enabled);
        // After disable, predict returns None
        assert!(p.predict(b"x").is_none());
    }

    #[test]
    fn raw_mode_escape_disables_prediction() {
        let mut p = make_predictor();
        assert!(p.enabled);
        p.check_output_for_raw_mode(b"\x1b[?1049h");  // enter alt screen
        assert!(!p.enabled);
    }

    #[test]
    fn exit_alt_screen_re_enables_prediction() {
        let mut p = make_predictor();
        p.check_output_for_raw_mode(b"\x1b[?1049h");
        assert!(!p.enabled);
        p.check_output_for_raw_mode(b"\x1b[?1049l");
        assert!(p.enabled);
    }
}
```

### SessionMetrics

```rust
// src/metrics.rs

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn rtt_estimate_averages_samples() {
        let mut m = SessionMetrics::new(100);
        m.record_hit(Duration::from_millis(100));
        m.record_hit(Duration::from_millis(200));
        m.record_hit(Duration::from_millis(300));
        // average = 200ms
        assert_eq!(m.rtt_estimate(), Duration::from_millis(200));
    }

    #[test]
    fn prediction_accuracy_zero_when_no_samples() {
        let m = SessionMetrics::new(100);
        assert_eq!(m.prediction_accuracy(), 1.0); // vacuously perfect
    }

    #[test]
    fn prediction_accuracy_fraction() {
        let mut m = SessionMetrics::new(100);
        m.record_hit(Duration::from_millis(50));
        m.record_hit(Duration::from_millis(50));
        m.record_miss(Duration::from_millis(50));
        // 2 hits, 1 miss → 2/3
        assert!((m.prediction_accuracy() - 2.0/3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn rtt_ring_buffer_evicts_oldest() {
        let mut m = SessionMetrics::new(3); // capacity=3
        m.record_hit(Duration::from_millis(100));
        m.record_hit(Duration::from_millis(100));
        m.record_hit(Duration::from_millis(100));
        m.record_hit(Duration::from_millis(400)); // evicts first 100
        // ring now: [100, 100, 400] → avg = 200
        assert_eq!(m.rtt_estimate(), Duration::from_millis(200));
    }
}
```

---

## Integration Tests

Integration tests open real PTY pairs and verify end-to-end behavior.

```rust
// tests/integration/buffering.rs

use ptyx::buffer::InputBuffer;
use ptyx::pty::open_pty;
use std::io::{Read, Write};
use std::time::{Duration, Instant};

/// Verify that buffered bytes all arrive in one read on the slave side.
#[test]
fn buffer_delivers_batched_bytes() {
    let pty = open_pty().expect("openpty");
    let mut master = unsafe { std::fs::File::from_raw_fd(pty.master) };
    let mut slave  = unsafe { std::fs::File::from_raw_fd(pty.slave) };

    // Write 3 bytes through a buffer
    let mut buf = InputBuffer::new(Duration::from_millis(50), 512);
    buf.push(b'a');
    buf.push(b'b');
    buf.push(b'c');
    // Simulate deadline expiry
    let chunk = buf.take();
    master.write_all(&chunk).unwrap();

    // Read on slave side
    let mut received = [0u8; 16];
    slave.set_nonblocking(true).unwrap();
    let n = slave.read(&mut received).unwrap();
    assert_eq!(&received[..n], b"abc");
}

/// Verify that Enter causes immediate flush (no 20ms wait).
#[test]
fn enter_flushes_immediately() {
    let mut buf = InputBuffer::new(Duration::from_millis(500), 512);
    buf.push(b'l');
    buf.push(b's');
    let flush_now = buf.push_and_maybe_flush(b'\n');
    
    let t0 = Instant::now();
    assert!(flush_now, "Enter should trigger immediate flush");
    assert!(t0.elapsed() < Duration::from_millis(5)); // instant
}
```

```rust
// tests/integration/echo_predict.rs

use ptyx::predict::{EchoPredictor, ReconcileResult};

/// Simulate typing "ls\n" and getting correct echo back.
#[test]
fn full_cooked_mode_echo_roundtrip() {
    let mut p = EchoPredictor::new(3);

    // Predict "ls\n"
    let echo = p.predict(b"ls\n").unwrap();
    assert!(echo.contains("ls"));

    // Server echoes "ls\r\n"
    let result = p.reconcile(b"ls\r\n");
    assert!(
        matches!(result, ReconcileResult::Confirmed { .. }),
        "Cooked mode echo should confirm"
    );
}
```

---

## Benchmarks (Criterion)

```rust
// benches/buffer.rs

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use ptyx::buffer::InputBuffer;
use std::time::Duration;

fn bench_push_single_byte(c: &mut Criterion) {
    c.bench_function("InputBuffer::push single byte", |b| {
        b.iter(|| {
            let mut buf = InputBuffer::new(Duration::from_millis(20), 512);
            buf.push(black_box(b'x'));
        })
    });
}

fn bench_push_1000_bytes(c: &mut Criterion) {
    c.bench_function("InputBuffer::push 1000 bytes", |b| {
        b.iter(|| {
            let mut buf = InputBuffer::new(Duration::from_millis(20), 4096);
            for i in 0u16..1000 {
                buf.push(black_box(i as u8));
            }
        })
    });
}

fn bench_prediction_ascii(c: &mut Criterion) {
    use ptyx::predict::EchoPredictor;

    let inputs: &[&[u8]] = &[b"ls", b"pwd", b"echo hello world", b"cat /etc/hosts"];

    let mut group = c.benchmark_group("EchoPredictor::predict");
    for input in inputs {
        group.bench_with_input(
            BenchmarkId::from_parameter(input.len()),
            input,
            |b, input| {
                let mut predictor = EchoPredictor::new(3);
                b.iter(|| {
                    predictor.predict(black_box(input))
                })
            },
        );
    }
    group.finish();
}

fn bench_reconcile(c: &mut Criterion) {
    use ptyx::predict::EchoPredictor;

    c.bench_function("EchoPredictor::reconcile hit", |b| {
        b.iter(|| {
            let mut p = EchoPredictor::new(3);
            p.predict(b"hello");
            p.reconcile(black_box(b"hello"))
        })
    });
}

criterion_group!(benches, bench_push_single_byte, bench_push_1000_bytes, bench_prediction_ascii, bench_reconcile);
criterion_main!(benches);
```

---

## Running Tests

```bash
# All unit tests (fast, no PTY needed)
cargo test

# Integration tests (need PTY — Linux/macOS only)
cargo test --test '*'

# A specific test
cargo test test_buffer_flushing

# Benchmarks (requires --release)
cargo bench

# Benchmarks with baseline comparison
cargo bench --bench buffer -- --save-baseline before_optimization
# ... make changes ...
cargo bench --bench buffer -- --baseline before_optimization

# With coverage (requires cargo-llvm-cov)
cargo llvm-cov --lcov --output-path lcov.info
```

---

## Test Fixtures & Helpers

```rust
// tests/common/mod.rs — shared test utilities

use std::os::unix::io::RawFd;

pub struct TestPty {
    pub master: RawFd,
    pub slave: RawFd,
}

impl TestPty {
    pub fn new() -> Self {
        let pair = ptyx::pty::open_pty().expect("openpty in test");
        Self { master: pair.master, slave: pair.slave }
    }
}

impl Drop for TestPty {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.master);
            libc::close(self.slave);
        }
    }
}
```
