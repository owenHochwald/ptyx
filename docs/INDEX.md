# ptyx — Documentation Index

> **Agent read order:** Load this file first. Each entry lists its token budget and purpose so you can decide which to fetch next.

ptyx is a Rust-based PTY proxy wrapper that adds intelligent SSH input buffering, local echo prediction, and async reconciliation — reducing perceived latency on high-RTT SSH sessions.

---

## Files in This Directory

| File | Topic | ~Tokens | Read When |
|------|-------|---------|-----------|
| [00-overview.md](./00-overview.md) | Architecture, data flow, goals | 400 | First — always |
| [01-pty-fundamentals.md](./01-pty-fundamentals.md) | PTY pairs, kernel driver, modes | 600 | Working on PTY layer |
| [02-ssh-buffering.md](./02-ssh-buffering.md) | InputBuffer, flush rules, timing | 700 | Working on buffering |
| [03-echo-prediction.md](./03-echo-prediction.md) | EchoPredictor, reconciliation | 800 | Working on prediction |
| [04-async-patterns.md](./04-async-patterns.md) | tokio::select!, event loop | 500 | Working on async core |
| [05-terminal-control.md](./05-terminal-control.md) | Raw/cooked mode, SIGWINCH, size | 500 | Working on terminal I/O |
| [06-platform-impl.md](./06-platform-impl.md) | PTY creation, fork, crossterm | 600 | Working on platform code |
| [07-pitfalls.md](./07-pitfalls.md) | Anti-patterns, deadlocks, bugs | 400 | Before any PR review |
| [08-testing.md](./08-testing.md) | Unit, integration, benchmarks | 500 | Writing tests |
| [09-crates.md](./09-crates.md) | Dependency reference & rationale | 300 | Adding dependencies |
| [10-roadmap.md](./10-roadmap.md) | Build phases, milestones | 400 | Planning next phase |

---

## Quick Orientation

```
src/
├── main.rs          # Entry point, CLI arg parsing
├── proxy.rs         # PtyProxy orchestrator
├── buffer.rs        # InputBuffer — flush timing & batching
├── predict.rs       # EchoPredictor — local echo & reconciliation
├── display.rs       # Display — predicted echo output & correction
├── pty.rs           # PTY creation, fork, platform shims
├── terminal.rs      # Raw/cooked mode, SIGWINCH, crossterm
├── metrics.rs       # SessionMetrics — RTT, accuracy tracking
├── config.rs        # Config, FileConfig, RunMode, CLI merge logic
├── recorder.rs      # SessionRecorder — I/O logging to .ptyx files
└── replay.rs        # Session log parsing + async replay

docs/                # ← you are here
tests/               # Integration tests
benches/             # criterion benchmarks
```

---

## Planning Documents (in project root)

| File | Purpose |
|------|---------|
| [`TODO.md`](../TODO.md) | Master checklist — all phases, all tasks |
| [`PHASE1.md`](../PHASE1.md) | Detailed plan: scaffold + PTY + core buffering |
| [`PHASE2.md`](../PHASE2.md) | Detailed plan: adaptive buffering + metrics |

---

## Design Invariants (never break these)

1. **Proxy adds ≤ 2ms overhead** — buffering is bounded, not unbounded.
2. **Never corrupt the byte stream** — mispredictions must be correctable, not silently wrong.
3. **Raw mode = passthrough** — when remote app enables raw mode, bytes pass through untouched (no buffering, no prediction).
4. **Locks are never held across blocking I/O.**
5. **All I/O is async** — no `std::thread::sleep`, no blocking reads in the hot path.
6. **Backpressure is mandatory** — stop reading stdin when buffer is full; never let it grow unbounded.
