# ptyx — Master TODO

> Checked items are done. Phases are sequential; don't start a phase until the previous one's acceptance criteria pass.
> TDD rule: every checkbox under "Tests" must have a **failing test committed** before the implementation checkbox is started.

---

## Phase 1 — Scaffold + PTY Proxy + Core Buffering

Goal: `ptyx user@host` opens a working SSH session with 20ms input buffering.  
Status: ✅ Complete

### 1.0 Project Setup
- [x] `cargo init ptyx --edition 2021 --lib` (lib + thin binary)
- [x] Add all dependencies to `Cargo.toml`
- [x] Create `src/lib.rs` skeleton (`pub mod` declarations only)
- [x] Create `.github/workflows/ci.yml` — `cargo test`, `clippy -D warnings`, `fmt --check`

### 1.1 Config (src/config.rs)
**Tests first:**
- [x] `default_config_flush_interval_is_20ms`
- [x] `default_config_max_size_is_512`
- [x] `buffer_config_debug_impl`
- [x] `ssh_args_appends_target`

**Implement:**
- [x] `Config` struct with `#[derive(Debug)]`
- [x] `BufferConfig` sub-struct with `Default`
- [x] `Config::load_from_args()` via clap
- [x] `Config::ssh_args()` returns `Vec<String>` for subprocess

### 1.2 PTY Layer (src/pty.rs)
**Tests first:**
- [x] `open_pty_returns_valid_fds`
- [x] `pty_size_set_and_get_round_trips`
- [x] `open_pty_gives_distinct_fds`

**Implement:**
- [x] `PtyPair` struct (`master: OwnedFd`, `slave: OwnedFd`)
- [x] `open_pty() -> Result<PtyPair>`
- [x] `fork_ssh(pty: &PtyPair, args: &[String]) -> Result<Pid>`
- [x] `set_pty_size(fd: RawFd, rows: u16, cols: u16) -> Result<()>`
- [x] `get_terminal_size() -> Result<(u16, u16)>`
- [x] `wait_for_child(pid: Pid) -> Result<ExitStatus>`
- [x] Correct fd close after fork

### 1.3 Terminal Layer (src/terminal.rs)
**Tests first:**
- [x] `terminal_struct_is_send`
- [x] `terminal_drop_impl_exists`

**Implement:**
- [x] `Terminal` struct
- [x] `Terminal::enter() -> Result<Terminal>` — enable raw mode + panic hook
- [x] `impl Drop for Terminal` — disable raw mode (infallible, logs errors)
- [x] `Terminal::current_size() -> Result<(u16, u16)>`

### 1.4 Input Buffer (src/buffer.rs)
**Tests first (all pure logic — no PTY required):**
- [x] `empty_buffer_does_not_flush`
- [x] `single_byte_arms_deadline`
- [x] `deadline_expired_triggers_flush`
- [x] `max_size_triggers_flush`
- [x] `take_clears_buffer_and_returns_bytes`
- [x] `take_on_empty_returns_empty_vec`
- [x] `is_empty_true_initially`
- [x] `is_empty_false_after_push`
- [x] `len_tracks_data_bytes`
- [x] `enter_lf_is_immediate`
- [x] `enter_cr_is_immediate`
- [x] `ctrl_c_is_immediate`
- [x] `ctrl_d_is_immediate`
- [x] `ctrl_z_is_immediate`
- [x] `regular_char_not_immediate`
- [x] `nul_byte_not_immediate`
- [x] `push_and_maybe_flush_returns_true_on_enter`
- [x] `push_and_maybe_flush_returns_false_on_regular_char`
- [x] `push_and_maybe_flush_accumulates_before_enter`
- [x] `utf8_complete_two_byte_char_passes_through`
- [x] `utf8_incomplete_first_byte_held_back`
- [x] `utf8_completed_by_second_byte`
- [x] `utf8_three_byte_sequence_held_until_complete`

**Implement:**
- [x] `InputBuffer` struct
- [x] `InputBuffer::new(flush_interval, max_size)`
- [x] `InputBuffer::is_immediate(byte)`
- [x] `InputBuffer::push(&mut self, byte)`
- [x] `InputBuffer::push_and_maybe_flush(&mut self, byte) -> bool`
- [x] `InputBuffer::should_flush(&self) -> bool`
- [x] `InputBuffer::take(&mut self) -> Vec<u8>`
- [x] `InputBuffer::is_empty/len/deadline`
- [x] UTF-8 carry-over: `has_incomplete_utf8()`

### 1.5 Event Loop (src/proxy.rs)
**Tests first:**
- [x] Integration: `buffer_delivers_batched_bytes_to_pty`
- [x] Integration: `enter_flushes_immediately_no_20ms_wait`
- [x] Integration: `ctrl_c_passes_through_immediately`
- [x] Integration: `ctrl_d_passes_through_immediately`
- [x] Integration: `pty_proxy_type_is_sized`

**Implement:**
- [x] `PtyProxy` struct (holds `Terminal`, `InputBuffer`, master fd, child `Pid`)
- [x] `PtyProxy::new(config: Config) -> Result<PtyProxy>`
- [x] `PtyProxy::run(self) -> Result<()>` — `tokio::select!` loop
- [x] SIGWINCH handler → `set_pty_size`
- [x] SIGTERM/SIGHUP → clean shutdown
- [x] Child-exit detection → restore terminal + exit

### 1.6 Main Entry (src/main.rs ≤ 50 lines)
- [x] `Config::load_from_args()`
- [x] `tracing_subscriber` init
- [x] `tokio::runtime::Runtime::new()?.block_on(PtyProxy::new(config)?.run())`

### 1.7 Benchmarks (baseline before Phase 2)
- [x] `benches/buffer.rs` — `bench_push_single_byte`, `bench_push_1000_bytes`, `bench_take`
- [x] Save baseline: `cargo bench --bench buffer -- --save-baseline phase1`

### Phase 1 Acceptance Criteria
- [ ] `ptyx user@localhost` opens SSH, types work, Ctrl+D exits cleanly (manual)
- [x] `cargo test` — all green (41 tests)
- [x] `cargo test --test '*'` — all green
- [x] `cargo clippy -- -D warnings` — zero warnings
- [x] `cargo fmt --check` — clean
- [x] No `.unwrap()` outside `#[cfg(test)]`
- [x] No `println!` / `eprintln!` in non-test code

---

## Phase 2 — Buffering Excellence + Metrics

Goal: Adaptive flush timing, binary protocol bypass, backpressure, raw mode passthrough, session metrics, live stats display.  
Status: 🔴 Not started (blocked on Phase 1)

### 2.1 Session Metrics (src/metrics.rs)
**Tests first:**
- [ ] `rtt_estimate_averages_samples`
- [ ] `prediction_accuracy_zero_when_no_samples`
- [ ] `prediction_accuracy_correct_fraction`
- [ ] `rtt_ring_buffer_evicts_oldest`
- [ ] `bytes_saved_accumulates_correctly`
- [ ] `buffer_depth_tracks_current_pending`

**Implement:**
- [ ] `SessionMetrics` struct with ring buffer for RTT samples
- [ ] `record_flush(bytes: usize, batch_size: usize)` — tracks bytes-saved vs one-at-a-time
- [ ] `record_rtt(rtt: Duration)`
- [ ] `rtt_estimate() -> Duration` — rolling average
- [ ] `bytes_saved() -> u64` — cumulative (batched sends vs hypothetical unbatched)
- [ ] `buffer_depth() -> usize`

### 2.2 Advanced Buffering (src/buffer.rs additions)
**Tests first:**
- [ ] `adaptive_interval_decreases_on_low_rtt`
- [ ] `adaptive_interval_increases_on_high_rtt`
- [ ] `binary_mode_bypasses_all_buffering`
- [ ] `raw_mode_passthrough_skips_buffer`
- [ ] `backpressure_blocks_input_when_buffer_full`
- [ ] `backpressure_releases_after_flush`
- [ ] Property test: `prop_flush_never_splits_utf8` (proptest)
- [ ] Property test: `prop_immediate_bytes_never_delayed`
- [ ] Property test: `prop_take_returns_all_pushed_bytes`

**Implement:**
- [ ] `InputBuffer::set_adaptive_interval(&mut self, rtt: Duration)` — adjusts flush window
- [ ] `InputBuffer::set_passthrough(&mut self, enabled: bool)` — raw mode bypass
- [ ] `InputBuffer::set_binary_mode(&mut self, enabled: bool)` — scp/sftp bypass
- [ ] Backpressure: `InputBuffer::is_full() -> bool`; proxy pauses stdin reads when true
- [ ] Binary mode detection heuristic (non-UTF-8 density threshold, or explicit flag from CLI)

### 2.3 CLI Enhancements
**Tests first:**
- [ ] `stats_flag_parsed_correctly`
- [ ] `buffer_interval_override_from_cli`
- [ ] `binary_mode_flag_parsed`

**Implement:**
- [ ] `--stats` flag: render live metrics bar (crossterm) at bottom of screen
- [ ] `--buffer <ms>` / `-b <ms>`: override default 20ms interval
- [ ] `--max-size <bytes>` / `-s <bytes>`: override 512B max
- [ ] `--no-buffer`: passthrough mode (for debugging / scp)
- [ ] `--verbose` / `-v`: enable debug tracing output

### 2.4 Wire Metrics into Proxy
- [ ] Record RTT on every PTY read (time between flush and first response byte)
- [ ] Record bytes-saved on every batch flush
- [ ] Pass metrics handle to buffer for depth tracking
- [ ] Stats renderer updates at ~4Hz (crossterm, non-blocking)

### 2.5 Benchmarks (compare vs Phase 1 baseline)
- [ ] `bench_push_single_byte` — verify ≤ 500ns, no regression vs phase1
- [ ] `bench_push_1000_bytes` — verify ≤ 500µs total
- [ ] `bench_adaptive_interval_update` — < 200ns
- [ ] `bench_passthrough_overhead` — confirm passthrough adds < 100ns vs direct write
- [ ] Run: `cargo bench -- --baseline phase1` and include output in PR

### Phase 2 Acceptance Criteria
- [ ] `ptyx --stats user@host` shows live RTT + bytes-saved
- [ ] `ptyx --no-buffer user@host` works (for scp / binary sessions)
- [ ] Buffer adapts flush interval based on observed RTT
- [ ] Raw mode (vim/htop) passes bytes through without buffering
- [ ] No benchmark regressions vs phase1 baseline
- [ ] All new tests green, clippy clean, fmt clean

---

## Phase 3 — Echo Prediction (Optional Enhancement)

Goal: Typed characters appear instantly in cooked mode; mispredictions corrected silently.  
Status: 🔴 Not started (blocked on Phase 2; de-prioritized — skip if project goals are met with Phase 2)

### 3.1 Echo Predictor (src/predict.rs)
**Tests first:** (all from `docs/08-testing.md` EchoPredictor section)
- [ ] `predicts_printable_ascii`
- [ ] `predicts_backspace_as_erase_sequence`
- [ ] `control_chars_not_echoed`
- [ ] `confirmed_reconcile_resets_miss_streak`
- [ ] `mispredicted_reconcile_increments_miss_streak`
- [ ] `prediction_disabled_after_threshold_misses`
- [ ] `raw_mode_escape_disables_prediction`
- [ ] `exit_alt_screen_re_enables_prediction`
- [ ] Integration: `full_cooked_mode_echo_roundtrip`
- [ ] `bench_prediction_ascii` baseline saved

**Implement:**
- [ ] `EchoPredictor`, `PendingInput`, `ReconcileResult`
- [ ] `predict(&mut self, input: &[u8]) -> Option<String>`
- [ ] `reconcile(&mut self, actual: &[u8]) -> ReconcileResult`
- [ ] `check_output_for_raw_mode(&mut self, output: &[u8])`
- [ ] Auto-disable after N consecutive misses

### 3.2 Display Layer (src/display.rs)
- [ ] Add `display.rs` to module-structure.md table and docs/INDEX.md
- [ ] `Display::write_predicted(&self, text: &str)`
- [ ] `Display::write_raw(&self, bytes: &[u8])`
- [ ] `Display::correct(&self, correction: &str)` — overwrite predicted text

### 3.3 Wire Prediction into Proxy
- [ ] Prediction only in cooked mode (not raw/binary)
- [ ] Disable prediction when `--no-predict` flag set
- [ ] Confirm prediction is off by default until Phase 3 stable

---

## Phase 4 — Config File + Session Recording

Goal: `~/.config/ptyx/config.toml` support; session replay; backend profiles.  
Status: 🔴 Not started (blocked on Phase 3 or skip to directly after Phase 2)

- [ ] TOML config: `[proxy]`, `[display]`, `[[backends]]` sections
- [ ] CLI args override config file values (merge with precedence)
- [ ] `SessionRecorder` plugin — logs all I/O to `~/.local/share/ptyx/sessions/`
- [ ] `ptyx replay <session.log>` subcommand
- [ ] `--config <path>` flag

---

## Ongoing (every phase)

- [ ] `docs/07-pitfalls.md` reviewed before every PR
- [ ] `cargo deny check` — license + advisory scan
- [ ] README.md kept current with each phase's features
- [ ] CLAUDE.md module table updated when new modules added
