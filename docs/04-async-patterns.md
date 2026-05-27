# Async Patterns

## Runtime Choice: tokio

ptyx uses `tokio` (multi-thread runtime) with `tokio::select!` as the central event dispatcher. Every I/O operation must be `async` — no blocking reads in the hot path.

## Core Event Loop

```rust
// src/proxy.rs — simplified event loop
pub async fn run(&mut self) -> anyhow::Result<()> {
    loop {
        tokio::select! {
            // ── 1. User keystrokes from stdin ──────────────────────
            result = self.stdin_reader.read_byte() => {
                let byte = result.context("stdin closed")?;
                let flush_now = self.buffer.push_and_maybe_flush(byte);
                
                // Predict echo (cooked mode only)
                if let Some(echo) = self.predictor.predict(&[byte]) {
                    self.display.write(&echo)?;
                }
                
                if flush_now {
                    let chunk = self.buffer.take();
                    self.pty_master.write_all(&chunk).await?;
                }
            }

            // ── 2. Server output from PTY master ───────────────────
            result = self.pty_master.read_chunk() => {
                let chunk = result.context("PTY master closed")?;
                
                // Check if remote app changed modes
                self.predictor.check_output_for_raw_mode(&chunk);
                
                match self.predictor.reconcile(&chunk) {
                    ReconcileResult::Confirmed { rtt } => {
                        self.metrics.record_hit(rtt);
                        // Display already correct — nothing to do
                    }
                    ReconcileResult::Mispredicted { rtt, correction } => {
                        self.metrics.record_miss(rtt);
                        self.display.correct(&correction)?;
                    }
                    ReconcileResult::Passthrough => {
                        self.display.write_raw(&chunk)?;
                    }
                }
            }

            // ── 3. Buffer flush deadline ────────────────────────────
            _ = tokio::time::sleep_until(self.buffer.deadline()), if !self.buffer.is_empty() => {
                let chunk = self.buffer.take();
                self.pty_master.write_all(&chunk).await?;
            }

            // ── 4. Signal channel ───────────────────────────────────
            Some(sig) = self.signal_rx.recv() => {
                match sig {
                    Signal::Winch => self.handle_resize()?,
                    Signal::Term | Signal::Hup => {
                        self.flush_and_shutdown().await?;
                        return Ok(());
                    }
                }
            }

            // ── 5. Child process exit ───────────────────────────────
            status = self.child_watcher.wait() => {
                tracing::info!("ssh exited: {:?}", status);
                return Ok(());
            }
        }
    }
}
```

## Key Rules

1. **`select!` branches are polled fairly** — no branch starves.
2. **No `.unwrap()` in async branches** — use `?` with `context()`.
3. **Futures inside `select!` must be cancel-safe** — `tokio::io::AsyncReadExt::read()` is cancel-safe; `Mutex::lock().await` inside `select!` is risky.
4. **Keep each branch <1ms** — CPU work inside `select!` blocks all other branches.

## Spawn vs Select

| Pattern | Use when |
|---------|----------|
| `tokio::select!` | Low count of concurrent I/O sources (our case: 5) |
| `tokio::spawn` | Long-running independent tasks (metrics flush, reconnect) |
| `JoinSet` | Waiting on N spawned tasks with cleanup |

Heavy work (compression, TLS re-keying) should be spawned, not inlined.

## Async I/O Wrappers

```rust
// Wrap raw fd in tokio AsyncFd for non-blocking PTY reads
use tokio::io::unix::AsyncFd;

let async_master = AsyncFd::new(pty_master_fd)?;

// Non-blocking read
let mut guard = async_master.readable().await?;
guard.try_io(|inner| inner.get_ref().read(&mut buf))?;
```

## Backpressure

If `pty_master.write_all()` blocks (SSH channel window full), `select!` won't service stdin until the write completes. This is correct — we must not accept more input than SSH can buffer. The local buffer acts as a small shock absorber.

## Timeout Patterns

```rust
// Reconnect with timeout
tokio::time::timeout(
    Duration::from_secs(30),
    reconnect(),
).await
.context("reconnect timed out")?
.context("reconnect failed")?;
```

## Task Shutdown

```rust
// Use CancellationToken for clean shutdown
use tokio_util::sync::CancellationToken;

let token = CancellationToken::new();

tokio::select! {
    _ = token.cancelled() => { /* shutdown */ }
    _ = do_work() => {}
}
```
