# Platform Implementation

## PTY Creation

### Linux / macOS (via `nix`)

```rust
// src/pty.rs

use nix::pty::{openpty, PtyMaster};
use nix::unistd::{execvp, ForkResult, fork};
use nix::sys::termios;
use std::ffi::CString;
use std::os::unix::io::{FromRawFd, IntoRawFd, RawFd};

pub struct PtyPair {
    pub master: RawFd,
    pub slave: RawFd,
}

pub fn open_pty() -> anyhow::Result<PtyPair> {
    let result = openpty(None, None)?;
    Ok(PtyPair {
        master: result.master.into_raw_fd(),
        slave:  result.slave.into_raw_fd(),
    })
}
```

### Forking the SSH Child

```rust
pub struct ChildProcess {
    pub pid: nix::unistd::Pid,
}

/// Fork, connect child to PTY slave, exec ssh.
/// Returns in parent only.
pub fn fork_ssh(
    pty: &PtyPair,
    ssh_args: &[&str],
) -> anyhow::Result<ChildProcess> {
    use nix::unistd::{close, dup2, setsid};

    let pid = unsafe { fork() }?;

    match pid {
        ForkResult::Child => {
            // New session so we become the controlling terminal's foreground
            setsid()?;

            // Connect slave to stdio
            dup2(pty.slave, libc::STDIN_FILENO)?;
            dup2(pty.slave, libc::STDOUT_FILENO)?;
            dup2(pty.slave, libc::STDERR_FILENO)?;

            // Close all other fds (master, original slave)
            close(pty.master)?;
            if pty.slave > 2 { close(pty.slave)?; }

            // Set PTY as controlling terminal
            unsafe {
                libc::ioctl(libc::STDIN_FILENO, libc::TIOCSCTTY, 0);
            }

            // Exec ssh
            let args: Vec<CString> = ssh_args.iter()
                .map(|s| CString::new(*s).unwrap())
                .collect();
            execvp(&args[0], &args)?;
            unreachable!()
        }

        ForkResult::Parent { child } => {
            close(pty.slave)?; // Parent doesn't need slave fd
            Ok(ChildProcess { pid: child })
        }
    }
}
```

### Waiting for Child Exit (Async)

```rust
use tokio::signal::unix::{signal, SignalKind};

pub async fn wait_for_child(pid: nix::unistd::Pid) -> anyhow::Result<i32> {
    let mut sigchld = signal(SignalKind::child())?;
    loop {
        sigchld.recv().await;
        match nix::sys::wait::waitpid(pid, Some(nix::sys::wait::WaitPidFlag::WNOHANG))? {
            nix::sys::wait::WaitStatus::Exited(_, code) => return Ok(code),
            nix::sys::wait::WaitStatus::Signaled(_, sig, _) => {
                return Ok(-(sig as i32))
            }
            _ => continue,
        }
    }
}
```

## PTY Size Ioctl

```rust
use nix::pty::Winsize;

pub fn set_pty_size(master_fd: RawFd, rows: u16, cols: u16) -> anyhow::Result<()> {
    let ws = Winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
    
    // SAFETY: fd is valid, ws is properly initialized
    unsafe {
        if libc::ioctl(master_fd, libc::TIOCSWINSZ, &ws) != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    Ok(())
}

pub fn get_pty_size(master_fd: RawFd) -> anyhow::Result<(u16, u16)> {
    let mut ws = Winsize { ws_row: 0, ws_col: 0, ws_xpixel: 0, ws_ypixel: 0 };
    unsafe {
        if libc::ioctl(master_fd, libc::TIOCGWINSZ, &mut ws) != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    Ok((ws.ws_row, ws.ws_col))
}
```

## Making the PTY Master Async

Raw file descriptors are blocking by default. Wrap in `tokio`'s `AsyncFd`:

```rust
use std::os::unix::io::AsRawFd;
use tokio::io::unix::AsyncFd;

pub struct AsyncPtyMaster {
    inner: AsyncFd<std::fs::File>,
}

impl AsyncPtyMaster {
    pub fn new(fd: RawFd) -> anyhow::Result<Self> {
        // Set non-blocking
        let flags = nix::fcntl::fcntl(fd, nix::fcntl::FcntlArg::F_GETFL)?;
        nix::fcntl::fcntl(
            fd,
            nix::fcntl::FcntlArg::F_SETFL(
                nix::fcntl::OFlag::from_bits_truncate(flags) | nix::fcntl::OFlag::O_NONBLOCK
            ),
        )?;

        let file = unsafe { std::fs::File::from_raw_fd(fd) };
        Ok(Self { inner: AsyncFd::new(file)? })
    }

    pub async fn read(&self, buf: &mut [u8]) -> anyhow::Result<usize> {
        loop {
            let mut guard = self.inner.readable().await?;
            match guard.try_io(|f| {
                use std::io::Read;
                f.get_ref().read(buf)  // Note: use get_ref() not get_mut()
                // Actually need raw read — see note below
            }) {
                Ok(result) => return Ok(result?),
                Err(_would_block) => continue,
            }
        }
    }

    pub async fn write_all(&self, buf: &[u8]) -> anyhow::Result<()> {
        let mut written = 0;
        while written < buf.len() {
            let mut guard = self.inner.writable().await?;
            match guard.try_io(|f| {
                use std::io::Write;
                f.get_ref().write(&buf[written..])
                // Same: needs raw write syscall
            }) {
                Ok(Ok(n)) => written += n,
                Ok(Err(e)) => return Err(e.into()),
                Err(_would_block) => continue,
            }
        }
        Ok(())
    }
}
```

## Config Loading

```rust
// src/config.rs

use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
pub struct Config {
    #[serde(default = "default_flush_ms")]
    pub flush_interval_ms: u64,
    
    #[serde(default = "default_buffer_size")]
    pub max_buffer_size: usize,
    
    #[serde(default = "default_prediction")]
    pub enable_prediction: bool,
    
    #[serde(default)]
    pub log_level: LogLevel,
    
    pub ssh: SshConfig,
}

#[derive(Deserialize, Debug, Clone)]
pub struct SshConfig {
    pub host: String,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity_file: Option<String>,
}

fn default_flush_ms() -> u64 { 20 }
fn default_buffer_size() -> usize { 512 }
fn default_prediction() -> bool { true }

impl Config {
    pub fn load(path: &std::path::Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        toml::from_str(&text).map_err(Into::into)
    }

    pub fn ssh_args(&self) -> Vec<String> {
        let mut args = vec!["ssh".to_string()];
        if let Some(port) = self.ssh.port {
            args.extend(["-p".to_string(), port.to_string()]);
        }
        if let Some(ref key) = self.ssh.identity_file {
            args.extend(["-i".to_string(), key.clone()]);
        }
        if let Some(ref user) = self.ssh.user {
            args.push(format!("{}@{}", user, self.ssh.host));
        } else {
            args.push(self.ssh.host.clone());
        }
        args
    }
}
```
