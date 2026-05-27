<h1 align="center">ptyx</h1>
<p align="center">Low-latency PTY proxy for SSH with intelligent input buffering and optional live stats.</p>
<p align="center">
  <a href="https://github.com/owenHochwald/ptyx/actions/workflows/ci.yml">
    <img alt="CI" src="https://github.com/owenHochwald/ptyx/actions/workflows/ci.yml/badge.svg">
  </a>
</p>

## Overview
ptyx wraps the system `ssh` binary in a local PTY proxy to reduce perceived latency on high-RTT links. It batches keystrokes for a short window (default 20ms / 512B), flushes immediately on control keys, adapts buffering to measured RTT, and can show a live metrics bar.

## How it works (mini diagram)
```
User terminal
  │ keystrokes
  ▼
InputBuffer (20ms/512B, adaptive)
  │ batched writes
  ▼
PTY master ──► ssh ──► remote host
  ▲                     │
  └────── output ◄──────┘
```
**Description:** ptyx sits between your terminal and the `ssh` subprocess. It reads raw keystrokes, batches them briefly, writes to the PTY master, and streams the remote output back to your screen. When a full-screen app switches to raw/alt-screen mode, ptyx automatically switches to passthrough to avoid buffering.

## Setup (source build)
**Prereqs:** macOS or Linux, Rust (stable), and `ssh` available on `PATH`.

```bash
git clone https://github.com/owenHochwald/ptyx.git
cd ptyx
cargo build --release
```

## Quick start
```bash
# Basic usage
cargo run -- user@host

# Include extra ssh args
cargo run -- user@host -- -p 2222 -i ~/.ssh/id_ed25519
```

## Usage
```
ptyx [options] user@host [-- ssh-args...]
```

Key options:
1. `-b, --buffer <ms>`: override the flush interval (default 20ms)
2. `-s, --max-size <bytes>`: max buffer size before forced flush (default 512)
3. `--no-buffer`: passthrough mode (use for scp/sftp or binary sessions)
4. `--adaptive`: RTT-based adaptive flush interval
5. `--stats`: live metrics bar (RTT, bytes saved, flushes)
6. `-v, --verbose`: enable debug logging (sets `RUST_LOG=ptyx=debug` if unset)

## Development
```bash
cargo test
cargo test --test '*'
cargo clippy -- -D warnings
cargo fmt --check
```

## Roadmap (planned)
- Echo prediction with reconciliation (Phase 3)
- Session persistence and replay (Phase 4)

## License
MIT — see [LICENSE](./LICENSE).
