# Build Roadmap

> Each phase follows TDD: **write tests first, then implement until they pass.**

---

## Phase 1 — Project Scaffold & PTY Creation

**Goal:** `ptyx user@host` opens a working SSH session via a PTY proxy (no buffering yet).

### Tests to write first
- [ ] `test_open_pty_returns_valid_fds` — master and slave fds are > 0
- [ ] `test_pty_size_set_and_get` — `set_pty_size` round-trips correctly
- [ ] `test_fork_ssh_returns_pid` — fork returns nonzero child PID
- [ ] `test_child_exits_cleanly` — SIGTERM to child, wait returns exit code

### Implementation tasks
1. `cargo init ptyx --edition 2024`
2. Add dependencies (see `09-crates.md`)
3. `src/pty.rs` — `open_pty()`, `fork_ssh()`, `set_pty_size()`, `wait_for_child()`
4. `src/terminal.rs` — `Terminal` struct with `enter()`/`leave()` + `Drop`
5. `src/config.rs` — `Config::load()`, `ssh_args()`
6. `src/main.rs` — parse CLI args, load config, call `PtyProxy::run()`

### Acceptance criteria
```bash
ptyx user@localhost  # opens SSH, types work, Ctrl+D exits cleanly
```

---

## Phase 2 — Input Buffering

**Goal:** Keystrokes are buffered 20ms before sending; latency improvement measurable.

### Tests to write first
All tests in `docs/08-testing.md` under "InputBuffer" section, plus:
- [ ] `bench_push_single_byte` baseline recorded
- [ ] `bench_push_1000_bytes` baseline recorded
- [ ] `integration::buffer_delivers_batched_bytes`
- [ ] `integration::enter_flushes_immediately`

### Implementation tasks
1. `src/buffer.rs` — `InputBuffer` struct, `push()`, `take()`, `is_immediate()`, `push_and_maybe_flush()`
2. Wire into event loop (`src/proxy.rs`)
3. Add buffer deadline to `tokio::select!`
4. UTF-8 boundary handling

### Benchmark target
- `push` < 500ns per call
- `take` < 100ns

---

## Phase 3 — Echo Prediction

**Goal:** Typed characters appear instantly; mispredictions corrected silently.

### Tests to write first
All tests in `docs/08-testing.md` under "EchoPredictor" section, plus:
- [ ] `test_prediction_disabled_by_alt_screen_escape`
- [ ] `test_re_enabled_after_exit_alt_screen`
- [ ] `integration::full_cooked_mode_echo_roundtrip`
- [ ] `bench_prediction_ascii` baseline

### Implementation tasks
1. `src/predict.rs` — `EchoPredictor`, `predict()`, `reconcile()`, `check_output_for_raw_mode()`
2. `src/display.rs` — `Display`, `write_raw()`, `correct()`
3. Wire predictor into event loop
4. Detect raw mode via escape sequences
5. Auto-disable after miss threshold

### Benchmark target
- `predict()` < 1µs for 16-byte input
- `reconcile()` < 500ns for hit path

---

## Phase 4 — Metrics & Observability

**Goal:** `ptyx --stats` shows RTT, prediction accuracy, bytes saved.

### Tests to write first
All tests in `docs/08-testing.md` under "SessionMetrics" section.

### Implementation tasks
1. `src/metrics.rs` — `SessionMetrics`, ring buffer for RTT samples
2. Wire `record_hit(rtt)` and `record_miss(rtt)` into reconciler
3. `--stats` flag in CLI renders live metrics (crossterm)
4. Optional: JSON metrics export for dashboards

---

## Phase 5 — Session Persistence (Mosh-Inspired)

**Goal:** Brief network interruption doesn't kill the session.

### Tests to write first
- [ ] `test_reconnect_within_timeout_resumes_session`
- [ ] `test_session_state_serialized_and_restored`
- [ ] `test_pending_buffer_replayed_on_reconnect`

### Implementation tasks
1. Session state serialization (TOML or binary)
2. Reconnect loop with exponential backoff
3. Replay pending buffer on reconnect
4. `SIGHUP` triggers reconnect instead of exit

---

## Phase 6 — Plugin System

**Goal:** Users can add behavior (compression, logging, key remapping) without forking.

### Design
```rust
pub trait Plugin: Send + Sync {
    fn on_input(&mut self, bytes: &mut Vec<u8>);
    fn on_output(&mut self, bytes: &mut Vec<u8>);
    fn on_resize(&mut self, rows: u16, cols: u16);
}
```

Plugins loaded from shared libraries (`.so`/`.dylib`) or as compiled-in features via cargo features.

---

## Milestone Summary

| Phase | Deliverable | Tests Required |
|-------|-------------|----------------|
| 1 | Working SSH proxy | PTY creation, fork, config |
| 2 | Input buffering | Buffer unit + integration + bench |
| 3 | Echo prediction | Predictor unit + integration + bench |
| 4 | Metrics | Metrics unit + CLI output |
| 5 | Session persistence | Reconnect integration |
| 6 | Plugin system | Plugin trait contract tests |

---

## Definition of Done (per phase)

1. All planned tests pass (`cargo test`)
2. No regressions in existing tests
3. Benchmarks run without regression vs baseline (`cargo bench -- --baseline`)
4. `cargo clippy -- -D warnings` clean
5. `cargo fmt --check` clean
6. CLAUDE.md updated if new modules added
