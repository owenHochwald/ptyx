<h1 align="center">ptyx</h1>

<p align="center">
  A low-latency SSH wrapper for terminals on slow, high-RTT connections.
</p>

<p align="center">
  <a href="https://github.com/owenHochwald/ptyx/actions/workflows/ci.yml">
    <img alt="CI" src="https://github.com/owenHochwald/ptyx/actions/workflows/ci.yml/badge.svg">
  </a>
</p>

## What Problem Does It Solve?

SSH can feel sluggish on long-distance, VPN, satellite, or mobile links because every keystroke often waits on network round trips. `ptyx` improves perceived responsiveness by sitting between your terminal and `ssh`, buffering safe bursts of input for a few milliseconds, and flushing immediately when you press command-boundary keys such as Enter or Ctrl+C.

It keeps using your system `ssh`; it does not replace OpenSSH, require a server-side daemon, or change how you authenticate.

## How It Works

```text
your terminal -> ptyx input buffer -> local PTY -> ssh -> remote host
your terminal <- streamed output  <- local PTY <- ssh <- remote host
```

By default, `ptyx` batches input for up to 20ms or 512 bytes, whichever comes first. It automatically switches to passthrough for raw/full-screen terminal apps so programs like Vim, htop, and shells that manage their own display do not receive delayed or rewritten bytes.

Optional features include adaptive buffering, a live stats line, local echo prediction, session recording/replay, and reconnecting by starting a fresh SSH child after disconnect.

## Install

Prerequisites: macOS or Linux, Rust stable, and `ssh` on your `PATH`.

```bash
cargo install ptyx
```

From source:

```bash
git clone https://github.com/owenHochwald/ptyx.git
cd ptyx
cargo install --path .
```

## Usage

```bash
ptyx user@host
ptyx user@host -- -p 2222 -i ~/.ssh/id_ed25519
```

Useful options:

```bash
ptyx --adaptive user@host      # tune buffering from observed RTT
ptyx --stats user@host         # show RTT, bytes saved, and prediction accuracy
ptyx --no-buffer user@host     # pure passthrough mode
ptyx --record user@host        # save a .ptyx session log
ptyx replay session.ptyx       # replay recorded output
```

## Configuration

`ptyx` reads `~/.config/ptyx/config.toml` when present.

```toml
[proxy]
flush_interval_ms = 20
max_size = 512
adaptive = true

[display]
stats = true
predict = false

[persistence]
reconnect = true
reconnect_timeout_ms = 10000
```

CLI flags override the config file.

## Safety Notes

`--record` writes terminal input and output to disk. Do not enable it for sessions that may contain passwords, tokens, private keys, or other secrets.

`--reconnect` starts a fresh SSH child after a disconnect. It deliberately drops any input still buffered locally instead of replaying it into the new session, because replaying sensitive or context-dependent bytes across reconnects is unsafe.

## Development

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo test
cargo deny check
```

## License

MIT. See [LICENSE](./LICENSE).
