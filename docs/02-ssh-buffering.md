# SSH Input Buffering

## The Problem

SSH sends each keystroke as a separate TCP packet by default (Nagle's algorithm is often disabled for interactive sessions). On a 150ms RTT link:

```
"ls\n" = 3 keystrokes = 3 × 150ms = 450ms of round-trips before you see output
```

## The Solution: Deadline Buffering

Accumulate bytes for up to `flush_interval_ms` (default 20ms) **or** until `max_size` bytes accumulated, then flush in one write:

```
Keystroke 'l' → buffer (t=0ms)
Keystroke 's' → buffer (t=8ms)
Keystroke '\n'→ IMMEDIATE FLUSH (Enter always flushes)
→ single SSH write: "ls\n"
```

## InputBuffer Design

```rust
pub struct InputBuffer {
    data: Vec<u8>,
    deadline: Instant,
    flush_interval: Duration,  // e.g. Duration::from_millis(20)
    max_size: usize,           // e.g. 512 bytes
}

impl InputBuffer {
    pub fn push(&mut self, byte: u8) {
        if self.data.is_empty() {
            // Arm the deadline on first byte
            self.deadline = Instant::now() + self.flush_interval;
        }
        self.data.push(byte);
    }

    pub fn should_flush(&self) -> bool {
        !self.data.is_empty()
            && (self.data.len() >= self.max_size
                || Instant::now() >= self.deadline)
    }

    pub fn take(&mut self) -> Vec<u8> {
        self.deadline = Instant::now() + self.flush_interval;
        std::mem::take(&mut self.data)
    }
}
```

## Immediate-Flush Keys

Certain bytes **must bypass buffering** and flush the entire buffer immediately:

| Byte | Meaning | Why flush immediately |
|------|---------|----------------------|
| `0x0A` / `0x0D` | Enter (LF/CR) | Command boundary — server must see this promptly |
| `0x03` | Ctrl+C | SIGINT — latency-sensitive |
| `0x04` | Ctrl+D | EOF — must arrive in order |
| `0x1A` | Ctrl+Z | SIGTSTP |
| `0x5C` `0x03` | Ctrl+\ | SIGQUIT |

```rust
pub fn is_immediate(byte: u8) -> bool {
    matches!(byte, b'\n' | b'\r' | 0x03 | 0x04 | 0x1A)
}

pub fn push_and_maybe_flush(&mut self, byte: u8) -> bool {
    self.push(byte);
    if is_immediate(byte) {
        return true; // caller flushes now
    }
    self.should_flush()
}
```

## Flush Decision Tree

```
byte arrives
    │
    ├─ is_immediate(byte)? ──YES──► flush immediately
    │
    ├─ data.len() >= max_size? ──YES──► flush immediately
    │
    └─ NO ──► arm/check deadline
                  │
                  └─ deadline passed? ──YES──► flush
                                      NO ──► wait for next byte or timer
```

## Timer Integration (tokio)

```rust
// In the event loop (see 04-async-patterns.md)
tokio::select! {
    // Byte from user
    result = stdin_rx.recv() => {
        let byte = result?;
        let flush_now = buffer.push_and_maybe_flush(byte);
        predictor.predict(&[byte]);  // update local echo
        if flush_now {
            pty_master.write_all(&buffer.take()).await?;
        }
    }

    // Buffer deadline expired
    _ = tokio::time::sleep_until(buffer.deadline.into()), if !buffer.is_empty() => {
        pty_master.write_all(&buffer.take()).await?;
    }
}
```

## Tuning Parameters

| Parameter | Default | Notes |
|-----------|---------|-------|
| `flush_interval_ms` | 20 | Increase on very high-RTT links (>200ms) |
| `max_size` | 512 | Must be < SSH channel window size |
| `disable_buffering` | false | Per-session override (e.g. for scp) |

## UTF-8 Boundary Safety

Buffering must not split multi-byte UTF-8 sequences across flush boundaries. Track incomplete sequences:

```rust
pub struct InputBuffer {
    // ...
    utf8_incomplete: Vec<u8>,  // carry-over bytes of partial sequence
}

pub fn push_bytes(&mut self, bytes: &[u8]) {
    let combined = [self.utf8_incomplete.as_slice(), bytes].concat();
    let (complete, tail) = split_at_utf8_boundary(&combined);
    self.data.extend_from_slice(complete);
    self.utf8_incomplete = tail.to_vec();
}
```

## What Buffering Does NOT Do

- Does not reorder bytes — TCP order is preserved
- Does not coalesce Enter presses — each Enter flushes independently
- Does not buffer in raw mode (TUI apps) — bytes pass through immediately
- Does not buffer binary protocols (scp, sftp) — detect and bypass
