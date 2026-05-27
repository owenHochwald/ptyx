# Common Pitfalls & Anti-Patterns

> **Read before every PR.** These are the bugs that are easy to write and hard to find.

---

## 1. Blocking I/O in the Event Loop

❌ **Bad — freezes all other select! branches**
```rust
let byte = std::io::stdin().lock().bytes().next()??;  // BLOCKS
```

✅ **Good — yields to tokio scheduler**
```rust
let byte = stdin_reader.read_byte().await?;
```

---

## 2. Holding a Lock Across an Await Point

❌ **Bad — MutexGuard held across `.await`, tokio will panic in debug mode**
```rust
let guard = state.lock().unwrap();
pty_master.write_all(&data).await?;  // lock still held
drop(guard);
```

✅ **Good — release lock before awaiting**
```rust
let data = {
    let guard = state.lock().unwrap();
    guard.pending.clone()
};  // lock released here
pty_master.write_all(&data).await?;
```

---

## 3. Raw Signal Handlers with Async

❌ **Bad — undefined behavior; signal() is not async-safe**
```rust
unsafe {
    libc::signal(libc::SIGWINCH, handle_sigwinch as libc::sighandler_t);
}
```

✅ **Good — use signal-hook-tokio**
```rust
use signal_hook_tokio::Signals;
let mut signals = Signals::new([libc::SIGWINCH])?;
// Poll via tokio::select!
```

---

## 4. Assuming All Input Is ASCII

❌ **Bad — panics on multi-byte UTF-8**
```rust
let ch = byte as char;  // 0xC3 → garbage; 0x80 → panic on some paths
```

✅ **Good — accumulate bytes, decode at boundaries**
```rust
self.utf8_buf.push(byte);
if let Ok(s) = std::str::from_utf8(&self.utf8_buf) {
    // complete sequence
    self.utf8_buf.clear();
    process(s);
}
// else: wait for more bytes
```

---

## 5. Prompt Detection by String Matching

❌ **Bad — breaks with every shell and every theme**
```rust
if output.ends_with("$ ") || output.ends_with("# ") {
    // assume new prompt — wrong constantly
}
```

✅ **Good — don't detect prompts; use buffer-flush on Enter instead**
The user pressing Enter is the only reliable command boundary signal. Buffer until Enter, then treat all subsequent output as the command's response.

---

## 6. Not Restoring Terminal on Panic

❌ **Bad — user's terminal is broken after a ptyx crash**
```rust
enable_raw_mode()?;
// ... panic somewhere ...
// raw mode never disabled
```

✅ **Good — `Drop` impl as safety net**
```rust
impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stdout(), LeaveAlternateScreen);
    }
}
```

Additionally, install a panic hook:
```rust
let original_hook = std::panic::take_hook();
std::panic::set_hook(Box::new(move |info| {
    let _ = disable_raw_mode();
    original_hook(info);
}));
```

---

## 7. Predicting in Raw Mode

❌ **Bad — TUI apps draw their own characters; your echo corrupts the screen**
```rust
// Always predict, regardless of mode
let echo = predictor.predict(&input);
display.write(&echo)?;
```

✅ **Good — disable prediction when raw mode detected**
```rust
if predictor.enabled {
    if let Some(echo) = predictor.predict(&input) {
        display.write(&echo)?;
    }
} else {
    // Raw mode: pass through without prediction
}
```

---

## 8. fd Leak After Fork

❌ **Bad — slave fd open in parent; master fd open in child**
```rust
let pty = open_pty()?;
fork_ssh(&pty, args)?;
// Forgot to close slave in parent and master in child
```

✅ **Good — explicit close after fork**
```rust
ForkResult::Child => {
    close(pty.master)?;
    // ... connect slave, exec ...
}
ForkResult::Parent { .. } => {
    close(pty.slave)?;  // Parent only needs master
    // ...
}
```

---

## 9. Unbounded Buffer Growth

❌ **Bad — network paused; buffer grows forever**
```rust
loop {
    let byte = read_stdin().await?;
    buffer.push(byte);  // no size limit
}
```

✅ **Good — enforce max_size; apply backpressure**
```rust
if buffer.len() >= MAX_BUFFER_SIZE {
    // Flush synchronously before accepting more input
    flush(&mut buffer, &mut pty_master).await?;
}
buffer.push(byte);
```

---

## 10. Reconcile Without Ordering Guarantee

❌ **Bad — assumes one chunk = one command's output**
```rust
let output = pty_master.read_chunk().await?;
reconcile_command(pending_commands.pop_front(), output);
```

✅ **Good — reconcile is stateful; output may span chunks or multiple commands**
Reconciliation must be a streaming state machine, not a one-shot match. Use sequence IDs and accumulate until a prompt or known boundary is detected. See `03-echo-prediction.md`.

---

## Summary Table

| Pitfall | Symptom | Fix |
|---------|---------|-----|
| Blocking I/O | Event loop hangs | `async`/`.await` everywhere |
| Lock across await | Deadlock or panic | Drop lock before awaiting |
| Raw signals | UB, missed signals | `signal-hook-tokio` |
| ASCII assumption | Corrupt output / panic | UTF-8 boundary accumulation |
| Prompt detection | Constant misfires | Use Enter as boundary |
| No terminal restore | Broken shell after crash | `Drop` + panic hook |
| Predicting in raw mode | Corrupted TUI | Check `predictor.enabled` |
| fd leak | Extra SIGHUP, zombies | Close unused fds post-fork |
| Unbounded buffer | OOM on slow network | `max_size` + backpressure |
| Naive reconcile | Wrong output attributed | Streaming state machine |
