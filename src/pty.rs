use anyhow::{Context, Result};
use nix::sys::wait::{waitpid, WaitStatus};
use nix::unistd::{dup2, execvp, fork, ForkResult, Pid};
use std::ffi::CString;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::io::RawFd;
use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;

/// A pair of open PTY file descriptors.
pub struct PtyPair {
    pub master: OwnedFd,
    pub slave: OwnedFd,
}

/// Open a new PTY pair via posix_openpt/grantpt/unlockpt/ptsname.
pub fn open_pty() -> Result<PtyPair> {
    unsafe {
        let master_fd = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
        if master_fd < 0 {
            return Err(anyhow::anyhow!(
                "posix_openpt: {}",
                std::io::Error::last_os_error()
            ));
        }
        if libc::grantpt(master_fd) != 0 {
            return Err(anyhow::anyhow!(
                "grantpt: {}",
                std::io::Error::last_os_error()
            ));
        }
        if libc::unlockpt(master_fd) != 0 {
            return Err(anyhow::anyhow!(
                "unlockpt: {}",
                std::io::Error::last_os_error()
            ));
        }
        let slave_name_ptr = libc::ptsname(master_fd);
        if slave_name_ptr.is_null() {
            return Err(anyhow::anyhow!("ptsname returned null"));
        }
        let slave_fd = libc::open(slave_name_ptr, libc::O_RDWR | libc::O_NOCTTY);
        if slave_fd < 0 {
            return Err(anyhow::anyhow!(
                "open slave: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(PtyPair {
            master: OwnedFd::from_raw_fd(master_fd),
            slave: OwnedFd::from_raw_fd(slave_fd),
        })
    }
}

/// Fork and exec `ssh` with the PTY slave as stdin/stdout/stderr.
/// Parent must drop `pty.slave` after calling this.
pub fn fork_ssh(pty: &PtyPair, args: &[String]) -> Result<Pid> {
    use std::os::unix::io::AsRawFd;
    let slave_fd = pty.slave.as_raw_fd();

    match unsafe { fork().context("fork")? } {
        ForkResult::Child => {
            nix::unistd::setsid().context("setsid")?;

            unsafe {
                libc::ioctl(slave_fd, libc::TIOCSCTTY as _, 0i32);
            }

            dup2(slave_fd, 0).context("dup2 stdin")?;
            dup2(slave_fd, 1).context("dup2 stdout")?;
            dup2(slave_fd, 2).context("dup2 stderr")?;

            if slave_fd > 2 {
                let _ = nix::unistd::close(slave_fd);
            }

            let ssh = CString::new("ssh").unwrap();
            let c_args: Vec<CString> = std::iter::once(ssh.clone())
                .chain(args.iter().map(|a| CString::new(a.as_str()).unwrap()))
                .collect();

            execvp(&ssh, &c_args).context("execvp ssh")?;
            unreachable!()
        }
        ForkResult::Parent { child } => Ok(child),
    }
}

/// Set PTY window size via TIOCSWINSZ ioctl.
pub fn set_pty_size(fd: RawFd, rows: u16, cols: u16) -> Result<()> {
    let ws = libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let ret = unsafe { libc::ioctl(fd, libc::TIOCSWINSZ, &ws) };
    if ret != 0 {
        return Err(anyhow::anyhow!(
            "TIOCSWINSZ: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

/// Get PTY window size via TIOCGWINSZ ioctl.
pub fn get_pty_size(fd: RawFd) -> Result<(u16, u16)> {
    let mut ws = libc::winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let ret = unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) };
    if ret != 0 {
        return Err(anyhow::anyhow!(
            "TIOCGWINSZ: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok((ws.ws_row, ws.ws_col))
}

/// Get current terminal size from stdout.
pub fn get_terminal_size() -> Result<(u16, u16)> {
    let (cols, rows) = crossterm::terminal::size().context("terminal::size")?;
    Ok((rows, cols))
}

/// PTY master fd whose reads and writes use raw syscalls — never calls lseek.
///
/// `tokio::fs::File` syncs its internal position between the async side and its
/// blocking thread by issuing `lseek` after reads, before writes. PTY character
/// devices return ESPIPE on any seek, causing "writing to PTY master / seek on
/// unseekable file" the first time a keystroke is flushed after SSH output arrives.
/// This type avoids that entirely by calling `libc::read`/`libc::write` directly
/// and setting the fd non-blocking for use with `tokio::io::unix::AsyncFd`.
#[derive(Debug)]
pub struct PtyMaster(OwnedFd);

impl PtyMaster {
    pub fn new(fd: OwnedFd) -> Result<Self> {
        let raw = fd.as_raw_fd();
        unsafe {
            let flags = libc::fcntl(raw, libc::F_GETFL);
            anyhow::ensure!(flags != -1, "fcntl F_GETFL: {}", std::io::Error::last_os_error());
            let ret = libc::fcntl(raw, libc::F_SETFL, flags | libc::O_NONBLOCK);
            anyhow::ensure!(ret != -1, "fcntl F_SETFL: {}", std::io::Error::last_os_error());
        }
        Ok(PtyMaster(fd))
    }
}

impl std::io::Read for &PtyMaster {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = unsafe {
            libc::read(self.0.as_raw_fd(), buf.as_mut_ptr() as *mut libc::c_void, buf.len())
        };
        if n < 0 { Err(std::io::Error::last_os_error()) } else { Ok(n as usize) }
    }
}

impl std::io::Write for &PtyMaster {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = unsafe {
            libc::write(self.0.as_raw_fd(), buf.as_ptr() as *const libc::c_void, buf.len())
        };
        if n < 0 { Err(std::io::Error::last_os_error()) } else { Ok(n as usize) }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl std::os::unix::io::AsRawFd for PtyMaster {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}

/// Wait for child and return its exit status.
pub fn wait_for_child(pid: Pid) -> Result<ExitStatus> {
    loop {
        match waitpid(pid, None).context("waitpid")? {
            WaitStatus::Exited(_, code) => return Ok(ExitStatus::from_raw(code)),
            WaitStatus::Signaled(_, sig, _) => return Ok(ExitStatus::from_raw(sig as i32)),
            _ => continue,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::io::AsRawFd;

    #[test]
    fn open_pty_returns_valid_fds() {
        let pair = open_pty().expect("open_pty");
        assert!(pair.master.as_raw_fd() > 0);
        assert!(pair.slave.as_raw_fd() > 0);
    }

    #[test]
    fn open_pty_gives_distinct_fds() {
        let pair = open_pty().expect("open_pty");
        assert_ne!(pair.master.as_raw_fd(), pair.slave.as_raw_fd());
    }

    #[test]
    fn pty_size_set_and_get_round_trips() {
        let pair = open_pty().expect("open_pty");
        let fd = pair.master.as_raw_fd();
        set_pty_size(fd, 24, 80).expect("set_pty_size");
        let (rows, cols) = get_pty_size(fd).expect("get_pty_size");
        assert_eq!(rows, 24);
        assert_eq!(cols, 80);
    }

    /// Regression: tokio::fs::File calls lseek after reads, causing ESPIPE on PTY.
    /// PtyMaster must be able to read then write without any seek in between.
    #[test]
    fn pty_master_write_does_not_fail_after_read() {
        let pair = open_pty().expect("open_pty");
        let slave_raw = pair.slave.as_raw_fd();

        // Put slave into non-blocking mode so we don't deadlock on echo reads.
        unsafe {
            let flags = libc::fcntl(slave_raw, libc::F_GETFL);
            libc::fcntl(slave_raw, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }

        let master = PtyMaster::new(pair.master).expect("PtyMaster::new");

        // Write bytes to the slave — the master will have data to read.
        let sent = unsafe { libc::write(slave_raw, b"hi\n".as_ptr() as _, 3) };
        assert_eq!(sent, 3, "slave write failed");

        // Read from master (simulates proxy consuming SSH output).
        let mut rbuf = [0u8; 16];
        let mut m: &PtyMaster = &master;
        let n = std::io::Read::read(&mut m, &mut rbuf).expect("read from master");
        assert!(n > 0, "expected data from master after slave write");

        // Write to master after the read — this is the sequence that caused ESPIPE.
        let mut m: &PtyMaster = &master;
        let written = std::io::Write::write(&mut m, b"ls\n").expect("write to master after read");
        assert_eq!(written, 3, "expected full write to PTY master");
    }
}
