# Build Roadmap

> Each phase follows TDD: **write tests first, then implement until they pass.**
> Detailed per-phase plans are in `PHASE1.md` and `PHASE2.md` at the project root.

---

## Phase 1 — Scaffold + PTY Proxy + Core Buffering

**Goal:** `ptyx user@host` opens a working SSH session with 20ms input buffering. No echo prediction.

**Priority: HIGH** — This is the core value prop. Everything else depends on it.

### Tests to write first
- [ ] All `InputBuffer` unit tests (pure logic — see `docs/08-testing.md` and `PHASE1.md`)
- [ ] `open_pty_returns_valid_fds`
- [ ] `pty_size_set_and_get`
- [ ] `fork_ssh_returns_pid`
- [ ] `child_exits_cleanly`
- [ ] Integration: `buffer_delivers_batched_bytes`
- [ ] Integration: `enter_flushes_immediately`

### Implementation tasks
1. `Cargo.toml` — all deps (see `docs/09-crates.md`)
2. `src/lib.rs` — module exports
3. `src/config.rs` — `Config`, `BufferConfig`, clap CLI
4. `src/pty.rs` — `open_pty()`, `fork_ssh()`, `set_pty_size()`, `wait_for_child()`
5. `src/terminal.rs` — `Terminal` with `enter()`/`Drop` + panic hook
6. `src/buffer.rs` — `InputBuffer`, `is_immediate()`, UTF-8 carry logic
7. `src/proxy.rs` — `PtyProxy`, `tokio::select!` event loop
8. `src/main.rs` — thin CLI wrapper (≤ 50 lines)

### Acceptance criteria
```bash
ptyx user@localhost  # opens SSH, types work, Ctrl+D exits cleanly
cargo bench -- --save-baseline phase1  # baselines saved for Phase 2 comparison
```

---

## Phase 2 — Buffering Excellence + Metrics

**Goal:** Adaptive timing, binary/raw passthrough, backpressure, session metrics, live stats display.

**Priority: HIGH** — Completes the buffering story before adding prediction complexity.

### Tests to write first
- [ ] All `SessionMetrics` unit tests (see `docs/08-testing.md`)
- [ ] `adaptive_interval_scales_with_rtt`
- [ ] `passthrough_mode_bypasses_deadline`
- [ ] `binary_mode_skips_utf8_check`
- [ ] `is_full_triggers_backpressure`
- [ ] Proptest suite for buffer invariants
- [ ] Integration: `raw_mode_output_triggers_passthrough`

### Implementation tasks
1. `src/metrics.rs` — `SessionMetrics`, RTT ring buffer, bytes-saved tracking
2. `src/buffer.rs` additions — `set_passthrough()`, `set_binary_mode()`, `is_full()`, `set_adaptive_interval()`
3. Wire metrics + backpressure into `src/proxy.rs`
4. Raw mode detection in output stream (`\x1b[?1049h` / `\x1b[?1049l`)
5. `--stats` flag + crossterm stats overlay
6. New CLI flags: `--buffer`, `--max-size`, `--no-buffer`, `--adaptive`, `--verbose`

### Benchmark target
- No regression > 10% vs phase1 baseline
- `set_adaptive_interval()` < 200ns
- `metrics.record_flush()` < 50ns

---

## Phase 3 — Echo Prediction (Optional Enhancement)

**Goal:** Typed characters appear instantly in cooked mode; mispredictions corrected silently.

**Priority: MEDIUM** — Skip if Phase 1+2 meet usability goals. High risk of display corruption if done wrong.

### Tests to write first
All tests in `docs/08-testing.md` under "EchoPredictor" section, plus:
- [ ] `test_prediction_disabled_by_alt_screen_escape`
- [ ] `test_re_enabled_after_exit_alt_screen`
- [ ] Integration: `full_cooked_mode_echo_roundtrip`
- [ ] `bench_prediction_ascii` baseline

### Implementation tasks
1. `src/predict.rs` — `EchoPredictor`, `predict()`, `reconcile()`, `check_output_for_raw_mode()`
2. `src/display.rs` — `Display`, `write_raw()`, `write_predicted()`, `correct()`
3. Wire predictor into event loop (disabled by default until stable)
4. Detect raw mode via escape sequences
5. Auto-disable after miss threshold

### Benchmark target
- `predict()` < 1µs for 16-byte input
- `reconcile()` < 500ns for hit path

---

## Phase 4 — Config File + Session Recording

**Goal:** `~/.config/ptyx/config.toml` support; session replay; backend profiles.

**Priority: LOW** — Quality-of-life, not core functionality.

### Implementation tasks
1. TOML config: `[proxy]`, `[display]`, `[[backends]]` sections
2. CLI args override config file values
3. `SessionRecorder` — log all I/O to `~/.local/share/ptyx/sessions/`
4. `ptyx replay <session.log>` subcommand

---

## Phase 5 — Session Persistence (Mosh-Inspired)

**Goal:** Brief SSH child interruption can spawn a fresh SSH child and replay locally buffered input.

**Status:** Complete for client-side reconnect. This is not full mosh-style remote process resurrection.

### Tests to write first
- [x] `reconnect_within_timeout_resumes_session_policy`
- [x] `pending_buffer_replayed_on_reconnect`

### Implementation tasks
1. [x] Reconnect policy and timeout config
2. [x] Reconnect loop with exponential backoff
3. [x] Replay pending buffer on reconnect
4. [x] `SIGHUP` triggers reconnect when `--reconnect` is enabled

---

## Phase 6 — Plugin System

**Goal:** Users can add behavior (compression, logging, key remapping) without forking.

**Priority: VERY LOW** — Design after Phase 4 experience.

```rust
pub trait Plugin: Send + Sync {
    fn on_input(&mut self, bytes: &mut Vec<u8>);
    fn on_output(&mut self, bytes: &mut Vec<u8>);
    fn on_resize(&mut self, rows: u16, cols: u16);
}
```

---

## Milestone Summary

| Phase | Deliverable | Priority | Tests Required |
|-------|-------------|----------|----------------|
| 1 | Working SSH proxy + buffering | HIGH | Buffer unit + PTY + integration |
| 2 | Adaptive buffering + metrics | HIGH | Metrics unit + proptest + bench |
| 3 | Echo prediction | MEDIUM | Predictor unit + integration + bench |
| 4 | Config file + recording | LOW | Config unit + recorder integration |
| 5 | Session persistence | LOW | Reconnect integration |
| 6 | Plugin system | VERY LOW | Plugin trait contract |

---

## Definition of Done (per phase)

1. All planned tests pass (`cargo test` + `cargo test --test '*'`)
2. No regressions in existing tests
3. Benchmarks run without regression vs baseline (`cargo bench -- --baseline <prev>`)
4. `cargo clippy -- -D warnings` clean
5. `cargo fmt --check` clean
6. `CLAUDE.md` module table updated if new modules added
7. `docs/INDEX.md` updated if new docs added
