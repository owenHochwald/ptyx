# Terminal Control

## Responsibility Split

| Layer | Owned by | Library |
|-------|----------|---------|
| User's terminal (raw mode, alt screen) | ptyx process | `crossterm` |
| PTY slave terminal attributes | kernel PTY driver | `nix::pty` |
| Remote terminal attributes | remote shell | SSH protocol |

## Setting Up the User Terminal

```rust
// src/terminal.rs

use crossterm::terminal::{enable_raw_mode, disable_raw_mode};
use crossterm::execute;
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use std::io;

pub struct Terminal {
    raw_mode_active: bool,
    alternate_screen: bool,
}

impl Terminal {
    pub fn enter(&mut self) -> anyhow::Result<()> {
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen)?;
        self.raw_mode_active = true;
        self.alternate_screen = true;
        Ok(())
    }

    pub fn leave(&mut self) -> anyhow::Result<()> {
        if self.alternate_screen {
            execute!(io::stdout(), LeaveAlternateScreen)?;
        }
        if self.raw_mode_active {
            disable_raw_mode()?;
        }
        Ok(())
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        // Always restore — even on panic
        let _ = self.leave();
    }
}
```

**Why `Drop` matters:** If ptyx panics with raw mode active, the user's terminal is broken (no echo, no line processing). The `Drop` impl is a safety net on top of explicit `leave()` calls.

## SIGWINCH — Terminal Resize

```rust
use signal_hook_tokio::Signals;
use signal_hook::consts::signal::SIGWINCH;

pub fn setup_signals() -> anyhow::Result<impl Stream<Item = i32>> {
    Ok(Signals::new([SIGWINCH, libc::SIGTERM, libc::SIGHUP])?)
}

pub fn handle_resize(pty_master: RawFd) -> anyhow::Result<()> {
    let size = crossterm::terminal::size()?;  // (cols, rows)
    
    let ws = nix::pty::Winsize {
        ws_row: size.1,
        ws_col: size.0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    
    // Push new size into PTY — kernel notifies slave (ssh)
    nix::ioctl_write_ptr_bad!(
        tiocswinsz, libc::TIOCSWINSZ, nix::pty::Winsize
    );
    unsafe { tiocswinsz(pty_master, &ws)? };
    
    Ok(())
    // SSH library additionally sends "window-change" channel request to server
}
```

## Reading from Stdin (Async, Raw Mode)

In raw mode, each byte arrives immediately. We must not block:

```rust
use tokio::io::{AsyncReadExt, stdin};

pub struct StdinReader {
    inner: tokio::io::Stdin,
    buf: [u8; 256],
}

impl StdinReader {
    pub async fn read_byte(&mut self) -> anyhow::Result<u8> {
        let mut byte = [0u8; 1];
        self.inner.read_exact(&mut byte).await?;
        Ok(byte[0])
    }

    pub async fn read_chunk(&mut self) -> anyhow::Result<&[u8]> {
        let n = self.inner.read(&mut self.buf).await?;
        if n == 0 {
            anyhow::bail!("stdin EOF");
        }
        Ok(&self.buf[..n])
    }
}
```

## Writing to Stdout

```rust
use tokio::io::{AsyncWriteExt, stdout};

pub struct Display {
    inner: tokio::io::Stdout,
}

impl Display {
    pub async fn write_raw(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        self.inner.write_all(bytes).await?;
        self.inner.flush().await?;
        Ok(())
    }

    // Overwrite predicted text with actual correction
    pub async fn correct(&mut self, correction: &str) -> anyhow::Result<()> {
        use crossterm::{cursor, terminal, QueueableCommand};
        use std::io::Write;

        let mut out = Vec::new();
        // Erase current line and rewrite
        out.queue(cursor::MoveToColumn(0))?;
        out.queue(terminal::Clear(terminal::ClearType::CurrentLine))?;
        out.write_all(correction.as_bytes())?;
        self.inner.write_all(&out).await?;
        self.inner.flush().await?;
        Ok(())
    }
}
```

## Mode Detection Cheat Sheet

```rust
// Escape sequences that signal mode changes

// → Remote app entering raw/TUI mode
"\x1b[?1049h"   // Alternate screen ON  (vim, less, fzf)
"\x1b[?25l"     // Hide cursor          (most TUIs)
"\x1b[?1h"      // Application cursor keys

// → Remote app leaving raw/TUI mode
"\x1b[?1049l"   // Alternate screen OFF
"\x1b[?25h"     // Show cursor

// → Password prompt (echo off)
// No escape sequence — detected by reconcile miss streak
```

## Checklist: Terminal Setup Order

1. `enable_raw_mode()` — capture every byte
2. `EnterAlternateScreen` — keep user's scrollback clean
3. `setup_signals()` — SIGWINCH, SIGTERM, SIGHUP
4. `create_pty()` + `fork_ssh()` — spawn child
5. Push initial terminal size to PTY master
6. Enter event loop

Teardown (reverse order):
1. `SIGTERM` child / wait
2. `LeaveAlternateScreen`
3. `disable_raw_mode()`
