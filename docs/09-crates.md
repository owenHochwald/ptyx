# Crate Reference

## Core Dependencies

```toml
[dependencies]
# Async runtime — the event loop backbone
tokio = { version = "1", features = ["full"] }

# Unix PTY syscalls (openpty, forkpty, ioctl)
nix = { version = "0.29", features = ["pty", "process", "signal", "term"] }

# Cross-platform terminal control (raw mode, alternate screen, cursor)
crossterm = "0.28"

# Async-compatible Unix signal handling
signal-hook-tokio = { version = "0.3", features = ["futures-v0_3"] }
signal-hook = "0.3"

# Error handling with context chaining
anyhow = "1"

# Structured async-safe logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }

# Config deserialization
serde = { version = "1", features = ["derive"] }
toml = "0.8"

# CLI argument parsing
clap = { version = "4", features = ["derive"] }

# Async task cancellation
tokio-util = { version = "0.7", features = ["rt"] }

[dev-dependencies]
# Benchmarking
criterion = { version = "0.5", features = ["html_reports"] }

# Property-based testing (optional but recommended)
proptest = "1"
```

## Why Each Crate

| Crate | Alternatives Considered | Why This One |
|-------|------------------------|--------------|
| `tokio` | `async-std`, `smol` | Largest ecosystem, best `select!`, `AsyncFd` for raw fds |
| `nix` | `libc` directly | Safe wrappers, typed ioctls, no raw `unsafe` in business logic |
| `crossterm` | `termion` | Cross-platform (Linux + macOS + Windows someday), maintained |
| `signal-hook-tokio` | `tokio::signal` | More signals, works with SIGWINCH (not just Ctrl+C) |
| `anyhow` | `thiserror`, `eyre` | Application code (not library) — ergonomic, no boilerplate |
| `tracing` | `log` + `env_logger` | Structured spans, async-aware, compatible with tokio-console |
| `clap` | `pico-args`, `argh` | Feature-rich, derive macro, shell completions |
| `criterion` | `divan` | Industry standard, statistical analysis, HTML reports |

## Crate Feature Flags

```toml
[features]
default = ["prediction", "metrics"]

# Disable echo prediction (pass-through only mode)
prediction = []

# Enable session metrics collection
metrics = []

# Enable tokio-console support for async debugging
console = ["console-subscriber"]
```

## Version Policy

- Pin major versions in `Cargo.toml` (no `*`)
- Run `cargo update` monthly; review changelogs for `nix`, `tokio`
- `nix` has breaking changes between minor versions — read release notes carefully
- Use `cargo deny` to audit licenses and security advisories

## Optional / Future Crates

| Crate | Purpose | Phase |
|-------|---------|-------|
| `russh` | Pure-Rust SSH client (drop system `ssh`) | Phase 3 |
| `zstd` | Compress sessions for replay | Phase 3 |
| `bytes` | Zero-copy buffer slices | Phase 2 (if profiling shows alloc pressure) |
| `dashmap` | Concurrent session registry | Phase 4 (multi-session) |
| `tokio-console` | Live async task inspector | Debug builds |
