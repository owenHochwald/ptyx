# ptyx — Architecture Overview

## What ptyx Does

ptyx wraps an SSH session in a local PTY proxy that:
- **Buffers** keystrokes for up to ~20ms then batches them into a single SSH write
- **Predicts** the server's echo and displays it locally before the round-trip completes
- **Reconciles** predictions against actual server output, correcting silently on mismatch
- **Reconnects** by starting a fresh SSH child after disconnect when explicitly enabled

## Latency Model

```
Without ptyx
  User keystroke → SSH → [100-500ms RTT] → Server → echo back → display
  Each keystroke = 1 network round-trip

With ptyx
  User keystroke → [predict echo locally, display instantly]
                 → [buffer 20ms] → batch → SSH → Server
  Display feels instant; network round-trips reduced ~5-10×
```

Net benefit is proportional to RTT: minimal on LAN (<5ms), large on WAN (100ms+).

## Component Map

```
┌─────────────────────────────────────────────────────────┐
│                        ptyx proxy                        │
│                                                         │
│  stdin/stdout                                           │
│      ↓  ↑                                               │
│  ┌──────────┐    ┌────────────┐    ┌─────────────────┐  │
│  │ Terminal │    │InputBuffer │    │  EchoPredictor  │  │
│  │  layer   │───►│ (20ms/512B)│───►│  (cooked only)  │  │
│  │crossterm │    └────────────┘    └────────┬────────┘  │
│  └──────────┘          │                   │           │
│                         │ flush             │ predicted │
│                         ▼                   ▼           │
│                   ┌──────────┐    ┌─────────────────┐  │
│                   │  PTY     │    │   Reconciler    │  │
│                   │ master   │◄───│  (verify match) │  │
│                   │  fd      │    └─────────────────┘  │
│                   └──────────┘                         │
│                        │                               │
│                   PTY slave fd                         │
│                        │                               │
│                   ┌──────────┐                         │
│                   │ ssh      │  ← child process        │
│                   │ process  │                         │
│                   └──────────┘                         │
│                        │                               │
│                   [Network — RTT]                       │
│                        │                               │
│                   Remote SSH server                     │
└─────────────────────────────────────────────────────────┘
```

## Data Flow (happy path)

1. User presses key → `terminal::read_byte()`
2. Byte lands in `InputBuffer`
3. `EchoPredictor::predict()` renders predicted echo to screen
4. Timer or flush trigger fires → `buffer::flush()` writes to PTY master fd
5. SSH child reads from PTY slave, sends over network
6. Server response arrives → PTY master fd readable
7. `Reconciler::reconcile()` compares against prediction
8. If match: no-op. If mismatch: overwrite display with correct output.
9. `SessionMetrics` records RTT sample, prediction hit/miss.

Reconnect deliberately drops any bytes still buffered locally. Replaying stale input into a fresh SSH session can leak secrets or execute context-dependent commands in the wrong shell state.

## Key Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Async runtime | tokio | Best ecosystem, `select!` macro, stable |
| PTY syscalls | `nix` crate | Safe Rust wrapper over POSIX `openpty`/`forkpty` |
| Terminal control | `crossterm` | Cross-platform, covers raw mode + alternate screen |
| Signal handling | `signal-hook-tokio` | Async-compatible, no UB like raw `signal()` |
| Config format | TOML | Human-friendly, `serde` + `toml` crates |
| Error handling | `anyhow` | Ergonomic, context chaining, no boilerplate |
| Logging | `tracing` | Structured, async-safe, spans for latency profiling |

## Non-Goals (v1)

- Not a full SSH client — wraps the system `ssh` binary
- Not a terminal emulator — defers rendering to the user's terminal
- Not Mosh — reconnect starts a fresh SSH child and does not preserve remote process state
