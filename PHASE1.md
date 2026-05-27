# Phase 1 — Scaffold + PTY Proxy + Core Buffering

**Goal:** `ptyx user@host` opens a working SSH session via a PTY proxy with 20ms input buffering.  
No echo prediction. No metrics display. Just a working, well-tested proxy.

**Estimated effort:** 1–2 days of focused work

---

## What We're Building

```
stdin (raw) ──► InputBuffer (20ms/512B) ──► PTY master fd ──► ssh process
                                                              (fork+exec)
stdout       ◄──────────────────────────── PTY master fd
```

The proxy sits between the user's terminal and the `ssh` subprocess. It:
1. Reads raw keystrokes from stdin
2. Accumulates them in `InputBuffer` for up to 20ms
3. Flushes immediately on Enter, Ctrl+C, Ctrl+D, Ctrl+Z
4. Writes batched bytes to the PTY master fd
5. Passes PTY output directly back to stdout
6. Forwards SIGWINCH (terminal resize) to the PTY
7. Restores raw mode on exit or panic

---

## Files in This Phase

| File | Responsibility | Hard limit |
|------|----------------|------------|
| `Cargo.toml` | All dependencies pinned | — |
| `src/lib.rs` | `pub mod` exports | 30 lines |
| `src/main.rs` | CLI parse → `PtyProxy::run()` | 50 lines |
| `src/config.rs` | `Config`, `BufferConfig`, clap CLI | 100 lines |
| `src/pty.rs` | `open_pty`, `fork_ssh`, size ioctls, wait | 200 lines |
| `src/terminal.rs` | Raw mode, `Drop`, panic hook | 200 lines |
| `src/buffer.rs` | `InputBuffer` deadline/batch/UTF-8 logic | 200 lines |
| `src/proxy.rs` | `PtyProxy`, `tokio::select!` event loop | 250 lines |

---

## TDD Order

> Write each test and confirm it **fails to compile or fails at runtime** before writing the implementation.

### Step 1 — buffer.rs tests (pure logic, instant feedback)

These tests need no PTY, no tokio, no fork. Run them with `cargo test` in seconds.

```rust
// All go in src/buffer.rs under #[cfg(test)]

fn make_buffer() -> InputBuffer {
    InputBuffer::new(Duration::from_millis(20), 512)
}

// Group 1: flush conditions
fn empty_buffer_does_not_flush()
fn single_byte_arms_deadline()
fn deadline_expired_triggers_flush()          // use Duration::from_millis(0)
fn max_size_triggers_flush()                  // max_size=3, push 3 bytes

// Group 2: take semantics
fn take_clears_buffer_and_returns_bytes()
fn take_on_empty_returns_empty_vec()
fn is_empty_true_initially()
fn is_empty_false_after_push()
fn len_tracks_data_bytes()

// Group 3: is_immediate
fn enter_lf_is_immediate()
fn enter_cr_is_immediate()
fn ctrl_c_is_immediate()
fn ctrl_d_is_immediate()
fn ctrl_z_is_immediate()
fn regular_char_not_immediate()
fn nul_byte_not_immediate()

// Group 4: push_and_maybe_flush
fn push_and_maybe_flush_returns_true_on_enter()
fn push_and_maybe_flush_returns_false_on_regular_char()
fn push_and_maybe_flush_accumulates_before_enter()  // push 'l','s','\n' → take = "ls\n"

// Group 5: UTF-8 safety
fn utf8_complete_two_byte_char_passes_through()    // 0xC3 0xA9 = 'é'
fn utf8_incomplete_first_byte_held_back()          // push 0xC3 → has_incomplete_utf8() = true
fn utf8_completed_by_second_byte()                 // push 0xC3 then 0xA9 → not incomplete
fn utf8_three_byte_sequence_held_until_complete()  // e.g. '€' = 0xE2 0x82 0xAC
```

### Step 2 — config.rs tests

```rust
// In src/config.rs under #[cfg(test)]

fn default_config_flush_interval_is_20ms()
fn default_config_max_size_is_512()
fn buffer_config_debug_impl()                 // just assert!(format!("{:?}", cfg).len() > 0)
```

### Step 3 — pty.rs tests

These require a real PTY fd — run on Linux/macOS only.

```rust
// In src/pty.rs under #[cfg(test)]

fn open_pty_returns_valid_fds()               // master > 0, slave > 0
fn pty_size_set_and_get_round_trips()         // set 80×24, get back 80×24
fn open_pty_gives_distinct_fds()             // master != slave

// In tests/integration/pty.rs
fn fork_ssh_echo_and_wait()                  // fork "echo hello", collect output, wait
fn child_exits_cleanly_on_sigterm()
```

### Step 4 — terminal.rs tests

```rust
// In src/terminal.rs under #[cfg(test)]

fn terminal_struct_is_send()                 // static assertion: fn _assert_send() where Terminal: Send {}
fn terminal_drop_impl_exists()               // just construct and drop in a test
```

> Note: Verifying raw mode is actually disabled in the test process is tricky. The integration test `proxy_exits_cleanly` covers this end-to-end.

### Step 5 — integration tests

```rust
// tests/integration/buffer.rs

fn buffer_delivers_batched_bytes_to_pty()
// Open a real PTY pair. Write "abc" through InputBuffer to master.
// Read from slave. Assert received == b"abc" in one chunk.

fn enter_flushes_immediately_no_20ms_wait()
// Create buffer with 500ms interval. Push 'l','s','\n'.
// Assert push_and_maybe_flush('\n') = true and take() = b"ls\n"
// (timer never fires — verifies it's the Enter logic, not the timer)

fn ctrl_c_passes_through_immediately()
fn ctrl_d_passes_through_immediately()

// tests/integration/proxy.rs
fn pty_proxy_can_be_constructed_and_dropped()
fn terminal_restored_after_proxy_drop()
```

---

## Implementation Order

Follow tests-first strictly: write the test file stubs, run `cargo test` (expect compile errors or test failures), then implement.

### 1. Cargo.toml

```toml
[package]
name = "ptyx"
version = "0.1.0"
edition = "2024"

[[bin]]
name = "ptyx"
path = "src/main.rs"

[lib]
name = "ptyx"
path = "src/lib.rs"

[dependencies]
tokio        = { version = "1", features = ["full"] }
nix          = { version = "0.29", features = ["pty", "process", "signal", "term"] }
crossterm    = "0.28"
signal-hook-tokio = { version = "0.3", features = ["futures-v0_3"] }
signal-hook  = "0.3"
anyhow       = "1"
tracing      = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
serde        = { version = "1", features = ["derive"] }
toml         = "0.8"
clap         = { version = "4", features = ["derive"] }

[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }
proptest  = "1"

[[bench]]
name = "buffer"
harness = false
```

### 2. src/lib.rs

```rust
pub mod buffer;
pub mod config;
pub mod proxy;
pub mod pty;
pub mod terminal;
// Phase 2+:
// pub mod metrics;
// pub mod predict;
// pub mod display;
```

### 3. src/config.rs

Key types:
```rust
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Config {
    pub target: String,           // user@host
    pub extra_ssh_args: Vec<String>,
    pub buffer: BufferConfig,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct BufferConfig {
    pub flush_interval_ms: u64,   // default: 20
    pub max_size: usize,           // default: 512
}

impl Default for BufferConfig {
    fn default() -> Self {
        Self { flush_interval_ms: 20, max_size: 512 }
    }
}
```

Use `clap::Parser` derive for CLI. `Config::load_from_args()` parses argv and returns `anyhow::Result<Config>`.

`Config::ssh_args(&self) -> Vec<String>` returns the full args to pass to `ssh`.

### 4. src/pty.rs

```rust
pub struct PtyPair {
    pub master: OwnedFd,
    pub slave: OwnedFd,
}

pub fn open_pty() -> anyhow::Result<PtyPair>
pub fn fork_ssh(pty: &PtyPair, args: &[String]) -> anyhow::Result<Pid>
pub fn set_pty_size(fd: RawFd, rows: u16, cols: u16) -> anyhow::Result<()>
pub fn get_terminal_size() -> anyhow::Result<(u16, u16)>
pub fn wait_for_child(pid: Pid) -> anyhow::Result<ExitStatus>
```

Critical fd discipline (pitfall §8):
```rust
match unsafe { nix::unistd::fork()? } {
    ForkResult::Child => {
        drop(pty.master);          // child doesn't need master
        // dup2 slave → 0/1/2, exec ssh
    }
    ForkResult::Parent { child } => {
        drop(pty.slave);           // parent doesn't need slave
        Ok(child)
    }
}
```

### 5. src/terminal.rs

```rust
pub struct Terminal {
    _raw: (),  // zero-size marker; raw mode is OS state, not a stored value
}

impl Terminal {
    pub fn enter() -> anyhow::Result<Terminal> {
        // Install panic hook (see async-safety.md)
        // crossterm::terminal::enable_raw_mode()?;
        Ok(Terminal { _raw: () })
    }

    pub fn current_size() -> anyhow::Result<(u16, u16)> { ... }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
        // log error if it fails — don't panic in drop
    }
}
```

The panic hook goes in `enter()`:
```rust
let original = std::panic::take_hook();
std::panic::set_hook(Box::new(move |info| {
    let _ = crossterm::terminal::disable_raw_mode();
    original(info);
}));
```

### 6. src/buffer.rs

```rust
pub struct InputBuffer {
    data: Vec<u8>,
    deadline: Instant,
    flush_interval: Duration,
    max_size: usize,
    utf8_carry: Vec<u8>,         // partial multi-byte sequence carry-over
}
```

Key invariant: **`utf8_carry` bytes are never included in `data`**. They wait for completion.

`push(byte)`:
1. Append to `utf8_carry`
2. Try `std::str::from_utf8(&utf8_carry)`
3. If valid UTF-8: move all bytes into `data`, clear `utf8_carry`
4. If invalid but could be prefix of valid sequence: keep in `utf8_carry`
5. If irrecoverably invalid: treat as binary, flush `utf8_carry` directly into `data` (don't corrupt)
6. Arm deadline if `data` was empty before this push

`take()` returns `data` (never `utf8_carry`) and resets deadline.

### 7. src/proxy.rs

```rust
pub struct PtyProxy {
    config: Config,
    terminal: Terminal,
    buffer: InputBuffer,
    master: tokio::fs::File,    // AsyncFd wrapper over PTY master
    child_pid: Pid,
}
```

Event loop skeleton:
```rust
pub async fn run(mut self) -> anyhow::Result<()> {
    let mut signals = Signals::new([libc::SIGWINCH, libc::SIGTERM, libc::SIGHUP])?;
    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut input_byte = [0u8; 1];

    loop {
        tokio::select! {
            // User keystroke
            n = stdin.read(&mut input_byte) => {
                if n? == 0 { break; }
                let flush_now = self.buffer.push_and_maybe_flush(input_byte[0]);
                if flush_now {
                    let chunk = self.buffer.take();
                    self.master.write_all(&chunk).await
                        .context("writing to PTY master")?;
                }
            }

            // Buffer deadline fired
            _ = tokio::time::sleep_until(self.buffer.deadline().into()),
              if !self.buffer.is_empty() => {
                let chunk = self.buffer.take();
                self.master.write_all(&chunk).await
                    .context("flushing buffer on deadline")?;
            }

            // PTY output (remote → user)
            n = self.master.read(&mut output_buf) => {
                match n {
                    Ok(0) | Err(_) => break,  // child exited or closed
                    Ok(n) => {
                        stdout.write_all(&output_buf[..n]).await?;
                        stdout.flush().await?;
                    }
                }
            }

            // Signals
            Some(sig) = signals.next() => {
                match sig {
                    libc::SIGWINCH => {
                        let (rows, cols) = Terminal::current_size()?;
                        set_pty_size(self.master.as_raw_fd(), rows, cols)?;
                    }
                    libc::SIGTERM | libc::SIGHUP => break,
                    _ => {}
                }
            }
        }
    }

    wait_for_child(self.child_pid)?;
    Ok(())
    // Terminal::drop() fires here → raw mode restored
}
```

### 8. src/main.rs (≤ 50 lines)

```rust
use anyhow::Result;
use ptyx::{config::Config, proxy::PtyProxy};

fn main() -> Result<()> {
    let config = Config::load_from_args()?;
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(PtyProxy::new(config)?.run())
}
```

---

## Benchmarks (save baseline before Phase 2)

```bash
# After Phase 1 is complete and all tests pass:
cargo bench -- --save-baseline phase1
```

Bench file `benches/buffer.rs` must cover:
- `bench_push_single_byte` — target: < 500ns
- `bench_push_1000_bytes` — target: < 500µs total
- `bench_take` — target: < 100ns

---

## Acceptance Criteria

All must pass before Phase 2 begins:

```bash
cargo test                    # ✓ all unit tests green
cargo test --test '*'         # ✓ all integration tests green
cargo clippy -- -D warnings   # ✓ zero warnings
cargo fmt --check             # ✓ clean
cargo bench -- --save-baseline phase1  # ✓ baselines saved
```

Manual:
- [ ] `ptyx user@localhost` — SSH opens, interactive typing works
- [ ] Resize terminal window — remote terminal resizes (check with `stty size` on remote)
- [ ] Ctrl+C cancels current command
- [ ] Ctrl+D exits shell cleanly
- [ ] Kill proxy with SIGTERM — terminal raw mode restored (shell usable after)
- [ ] `RUST_LOG=ptyx=debug ptyx user@localhost` — debug output visible without corrupting session

---

## What Phase 1 Explicitly Does NOT Include

- ❌ Echo prediction (Phase 3)
- ❌ Metrics display / `--stats` flag (Phase 2)
- ❌ Adaptive flush intervals (Phase 2)
- ❌ Binary protocol bypass / `--no-buffer` (Phase 2)
- ❌ Config file (`~/.config/ptyx/config.toml`) — CLI only in Phase 1
- ❌ Session recording (Phase 4)
- ❌ Plugin system (Phase 4+)
