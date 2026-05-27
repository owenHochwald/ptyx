# PTY Fundamentals

## What a PTY Is

A PTY (pseudo-terminal) is a kernel-managed pipe pair that emulates a physical serial terminal:

```
┌──────────────┐         ┌──────────────┐
│  Master Side │◄───────►│  Slave Side  │
│  (proxy/app) │  PTY    │ (subprocess) │
└──────────────┘  pair   └──────────────┘
```

**Master fd** — the proxy owns this. Read from it to get subprocess output; write to it to send input.  
**Slave fd** — the subprocess (ssh) gets this as its `stdin`/`stdout`/`stderr`. It looks like a real terminal.

The kernel PTY driver lives between them and handles:

| Feature | Description |
|---------|-------------|
| Line discipline | Cooked mode buffering, Ctrl+C → SIGINT |
| Echo | Cooked mode automatically echoes input back |
| Signal generation | Ctrl+C, Ctrl+Z, Ctrl+\ mapped to signals |
| Terminal size | `TIOCSWINSZ`/`TIOCGWINSZ` ioctls |
| `VEOF` / `VERASE` | ^D for EOF, backspace handling |

## Cooked vs Raw Mode

### Cooked Mode (canonical)
- Input is line-buffered: Enter required to deliver to process
- Kernel echoes each character automatically
- Special chars (`^C`, `^D`, `^Z`) trigger signals/EOF
- **ptyx can predict echo here** — what you type = what server sends back

### Raw Mode
- Character-at-a-time delivery, no buffering
- No automatic echo — application draws its own cursor
- `^C` is just byte `0x03`, not a signal
- **ptyx cannot predict in raw mode** — pass bytes through untouched
- Triggered by: vim, nano, less, fzf, any TUI app

```rust
// Detecting mode transitions
// Remote app calls: tcsetattr(fd, TCSANOW, &termios_with_ICANON_off)
// The PTY line discipline changes — proxy observes via TIOCGETA responses
// In practice: detect by watching for escape sequences that TUIs emit on launch
```

## PTY Pair Creation

```rust
// POSIX: openpty() creates the pair without forking
let pty_pair = openpty(None, None)?;
// pty_pair.master: RawFd — proxy holds this
// pty_pair.slave:  RawFd — subprocess gets this

// forkpty() = openpty() + fork() in one call
// Child process automatically gets slave as its controlling terminal
match unsafe { forkpty(None, None) }? {
    ForkResult::Parent { child } => { /* proxy code */ }
    ForkResult::Child => { /* execvp("ssh", args) */ }
}
```

## Terminal Size

Size is tracked as `(rows, cols, xpixel, ypixel)` via `winsize` struct.

```rust
pub struct TerminalSize {
    pub rows: u16,
    pub cols: u16,
    pub xpixel: u16,  // pixel width  (often 0)
    pub ypixel: u16,  // pixel height (often 0)
}
```

- Set on master with `TIOCSWINSZ` ioctl
- Query on master with `TIOCGWINSZ` ioctl  
- SSH protocol carries size changes to the server via channel request `"window-change"`
- `SIGWINCH` fires in the proxy when the user's terminal resizes

## Signal Propagation

| Signal | Source | ptyx action |
|--------|--------|-------------|
| `SIGWINCH` | User resizes terminal | Read new size, `TIOCSWINSZ` on master, SSH sends to server |
| `SIGINT` | Ctrl+C | Write `0x03` to master fd (or flush buffer, send immediately) |
| `SIGTERM` | Kill proxy | Graceful shutdown: flush buffers, close PTY, wait for child |
| `SIGHUP` | Terminal closed | Reconnect logic (Phase 4) or graceful exit |

## PTY vs Pipe

| | PTY | Pipe |
|-|-----|------|
| Bidirectional | Single fd pair, both dirs | Need 2 pipes (stdin + stdout) |
| Terminal features | Full (echo, signals, cooked/raw) | None |
| `isatty()` | Returns true on slave | Returns false |
| Works with `ssh -t` | Yes | No — `ssh` refuses `-t` without a TTY |
| Works with `vim` | Yes | No — editors check `isatty()` |

ptyx requires a PTY (not a pipe) because `ssh` must see a terminal or it won't allocate one on the remote side, breaking all interactive tools.
