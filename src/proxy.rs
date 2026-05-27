use anyhow::{Context, Result};
use futures_util::StreamExt;
use nix::unistd::Pid;
use signal_hook::consts::{SIGHUP, SIGTERM, SIGWINCH};
use signal_hook_tokio::Signals;
use std::io;
use std::os::unix::io::AsRawFd;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::io::unix::AsyncFd;

use crate::buffer::InputBuffer;
use crate::config::Config;
use crate::pty::{fork_ssh, open_pty, set_pty_size, wait_for_child, PtyMaster};
use crate::terminal::Terminal;

pub struct PtyProxy {
    #[allow(dead_code)]
    config: Config,
    #[allow(dead_code)]
    terminal: Terminal,
    buffer: InputBuffer,
    master: AsyncFd<PtyMaster>,
    child_pid: Pid,
}

impl PtyProxy {
    pub fn new(config: Config) -> Result<PtyProxy> {
        let pty = open_pty().context("open_pty")?;

        let ssh_args = config.ssh_args();
        let child_pid = fork_ssh(&pty, &ssh_args).context("fork_ssh")?;

        // Parent closes slave; child already has it via dup2.
        drop(pty.slave);

        // PtyMaster sets non-blocking mode; AsyncFd registers with tokio's reactor.
        let master_fd = PtyMaster::new(pty.master).context("PtyMaster::new")?;
        let initial_raw = master_fd.as_raw_fd();
        let master = AsyncFd::new(master_fd).context("AsyncFd::new")?;

        let terminal = Terminal::enter().context("Terminal::enter")?;

        if let Ok((rows, cols)) = Terminal::current_size() {
            let _ = set_pty_size(initial_raw, rows, cols);
        }

        let buffer = InputBuffer::new(
            Duration::from_millis(config.buffer.flush_interval_ms),
            config.buffer.max_size,
        );

        Ok(PtyProxy {
            config,
            terminal,
            buffer,
            master,
            child_pid,
        })
    }

    pub async fn run(mut self) -> Result<()> {
        let mut signals = Signals::new([SIGWINCH, SIGTERM, SIGHUP]).context("Signals::new")?;
        let mut stdin = tokio::io::stdin();
        let mut stdout = tokio::io::stdout();
        let mut input_byte = [0u8; 1];
        let mut output_buf = vec![0u8; 4096];

        loop {
            let deadline = self.buffer.deadline();
            let buffer_nonempty = !self.buffer.is_empty();

            tokio::select! {
                n = stdin.read(&mut input_byte) => {
                    match n.context("stdin read")? {
                        0 => break,
                        _ => {
                            let flush_now = self.buffer.push_and_maybe_flush(input_byte[0]);
                            if flush_now {
                                let chunk = self.buffer.take();
                                write_all_to_master(&self.master, &chunk).await
                                    .context("writing to PTY master")?;
                            }
                        }
                    }
                }

                _ = tokio::time::sleep_until(deadline.into()), if buffer_nonempty => {
                    let chunk = self.buffer.take();
                    write_all_to_master(&self.master, &chunk).await
                        .context("flushing buffer on deadline")?;
                }

                n = async {
                    loop {
                        let mut guard = self.master.readable().await?;
                        match guard.try_io(|inner| {
                            let mut m: &PtyMaster = inner.get_ref();
                            std::io::Read::read(&mut m, &mut output_buf)
                        }) {
                            Ok(result) => break result,
                            Err(_would_block) => {}
                        }
                    }
                } => {
                    match n {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            stdout.write_all(&output_buf[..n]).await?;
                            stdout.flush().await?;
                        }
                    }
                }

                Some(sig) = signals.next() => {
                    match sig {
                        SIGWINCH => {
                            if let Ok((rows, cols)) = Terminal::current_size() {
                                let _ = set_pty_size(self.master.as_raw_fd(), rows, cols);
                                tracing::debug!(rows, cols, "SIGWINCH: resized PTY");
                            }
                        }
                        SIGTERM | SIGHUP => {
                            tracing::info!(sig, "received signal, shutting down");
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }

        let _ = wait_for_child(self.child_pid);
        Ok(())
    }
}

/// Write all bytes to the PTY master using readiness-based I/O (no lseek).
async fn write_all_to_master(master: &AsyncFd<PtyMaster>, chunk: &[u8]) -> io::Result<()> {
    let mut written = 0;
    while written < chunk.len() {
        let mut guard = master.writable().await?;
        match guard.try_io(|inner| {
            let mut m: &PtyMaster = inner.get_ref();
            std::io::Write::write(&mut m, &chunk[written..])
        }) {
            Ok(Ok(n)) => written += n,
            Ok(Err(e)) => return Err(e),
            Err(_would_block) => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_type_is_sized() {
        let _ = std::mem::size_of::<PtyProxy>();
    }
}
