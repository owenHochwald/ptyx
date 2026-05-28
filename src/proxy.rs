use anyhow::{Context, Result};
use crossterm::{cursor, execute, style, terminal};
use futures_util::StreamExt;
use nix::unistd::Pid;
use signal_hook::consts::{SIGHUP, SIGTERM, SIGWINCH};
use signal_hook_tokio::Signals;
use std::io;
use std::os::unix::io::AsRawFd;
use std::time::{Duration, Instant};
use tokio::io::unix::AsyncFd;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::buffer::InputBuffer;
use crate::config::Config;
use crate::display::Display;
use crate::metrics::SessionMetrics;
use crate::predict::{EchoPredictor, ReconcileResult};
use crate::pty::{fork_ssh, open_pty, set_pty_size, wait_for_child, PtyMaster};
use crate::recorder::SessionRecorder;
use crate::terminal::Terminal;

pub struct PtyProxy {
    config: Config,
    #[allow(dead_code)]
    terminal: Terminal,
    buffer: InputBuffer,
    master: AsyncFd<PtyMaster>,
    child_pid: Pid,
    metrics: SessionMetrics,
    last_flush_at: Option<Instant>,
    predictor: EchoPredictor,
    display: Display,
    recorder: Option<SessionRecorder>,
}

impl PtyProxy {
    pub fn new(config: Config) -> Result<PtyProxy> {
        let pty = open_pty().context("open_pty")?;

        let ssh_args = config.ssh_args();
        let child_pid = fork_ssh(&pty, &ssh_args).context("fork_ssh")?;

        drop(pty.slave);

        let master_fd = PtyMaster::new(pty.master).context("PtyMaster::new")?;
        let initial_raw = master_fd.as_raw_fd();
        let master = AsyncFd::new(master_fd).context("AsyncFd::new")?;

        let terminal = Terminal::enter().context("Terminal::enter")?;

        if let Ok((rows, cols)) = Terminal::current_size() {
            let _ = set_pty_size(initial_raw, rows, cols);
        }

        let mut buffer = InputBuffer::new(
            Duration::from_millis(config.buffer.flush_interval_ms),
            config.buffer.max_size,
        );
        buffer.set_passthrough(config.buffer.passthrough);
        buffer.set_adaptive(config.buffer.adaptive);

        let recorder = if config.record {
            Some(SessionRecorder::new().context("creating session recorder")?)
        } else {
            None
        };

        Ok(PtyProxy {
            config,
            terminal,
            buffer,
            master,
            child_pid,
            metrics: SessionMetrics::new(32),
            last_flush_at: None,
            predictor: EchoPredictor::new(3),
            display: Display::new(),
            recorder,
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
            let buffer_full = self.buffer.is_full();
            let show_stats = self.config.show_stats;

            tokio::select! {
                // Backpressure: stop reading stdin when the buffer is saturated.
                n = stdin.read(&mut input_byte), if !buffer_full => {
                    match n.context("stdin read")? {
                        0 => break,
                        _ => {
                            let flush_now = self.buffer.push_and_maybe_flush(input_byte[0]);
                            self.metrics.set_buffer_depth(self.buffer.len());
                            if flush_now {
                                let chunk = self.buffer.take();
                                let batch = chunk.len();
                                if let Some(ref mut rec) = self.recorder {
                                    let _ = rec.record_input(&chunk);
                                }
                                // Predict per-flush, only in cooked (non-passthrough) mode.
                                if self.config.predict && !self.buffer.is_passthrough() {
                                    if let Some(echo) = self.predictor.predict(&chunk) {
                                        if !echo.is_empty() {
                                            self.display.write_predicted(&echo).ok();
                                        }
                                    }
                                }
                                write_all_to_master(&self.master, &chunk).await
                                    .context("writing to PTY master")?;
                                self.metrics.record_flush(batch);
                                self.last_flush_at = Some(Instant::now());
                            }
                        }
                    }
                }

                _ = tokio::time::sleep_until(deadline.into()), if buffer_nonempty => {
                    let chunk = self.buffer.take();
                    let batch = chunk.len();
                    if let Some(ref mut rec) = self.recorder {
                        let _ = rec.record_input(&chunk);
                    }
                    // Predict timed-flush batches too.
                    if self.config.predict && !self.buffer.is_passthrough() {
                        if let Some(echo) = self.predictor.predict(&chunk) {
                            if !echo.is_empty() {
                                self.display.write_predicted(&echo).ok();
                            }
                        }
                    }
                    write_all_to_master(&self.master, &chunk).await
                        .context("flushing buffer on deadline")?;
                    self.metrics.record_flush(batch);
                    self.last_flush_at = Some(Instant::now());
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
                            let chunk = &output_buf[..n];
                            if let Some(ref mut rec) = self.recorder {
                                let _ = rec.record_output(chunk);
                            }

                            if contains_enter_raw(chunk) {
                                self.buffer.set_passthrough(true);
                                tracing::debug!("raw mode detected, buffer → passthrough");
                            } else if contains_exit_raw(chunk) {
                                self.buffer.set_passthrough(false);
                                tracing::debug!("raw mode exit, buffer → normal");
                            }

                            if self.config.predict {
                                self.predictor.check_output_for_raw_mode(chunk);
                                match self.predictor.reconcile(chunk) {
                                    ReconcileResult::Confirmed { rtt } => {
                                        self.metrics.record_hit(rtt);
                                        self.display.clear_predicted();
                                        self.last_flush_at = None;
                                        self.buffer.set_adaptive_interval(
                                            self.metrics.rtt_estimate(),
                                        );
                                    }
                                    ReconcileResult::Mispredicted { rtt, correction } => {
                                        self.metrics.record_miss(rtt);
                                        self.display.correct(&correction).ok();
                                        self.last_flush_at = None;
                                        self.buffer.set_adaptive_interval(
                                            self.metrics.rtt_estimate(),
                                        );
                                    }
                                    ReconcileResult::Passthrough => {
                                        stdout.write_all(chunk).await?;
                                        stdout.flush().await?;
                                        if let Some(flush_time) = self.last_flush_at.take() {
                                            let rtt = flush_time.elapsed();
                                            self.metrics.record_rtt(rtt);
                                            self.buffer.set_adaptive_interval(
                                                self.metrics.rtt_estimate(),
                                            );
                                        }
                                    }
                                }
                            } else {
                                stdout.write_all(chunk).await?;
                                stdout.flush().await?;
                                if let Some(flush_time) = self.last_flush_at.take() {
                                    let rtt = flush_time.elapsed();
                                    self.metrics.record_rtt(rtt);
                                    self.buffer.set_adaptive_interval(self.metrics.rtt_estimate());
                                }
                            }
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

                _ = tokio::time::sleep(Duration::from_millis(250)), if show_stats => {
                    render_stats_bar(&self.metrics);
                }
            }
        }

        let _ = wait_for_child(self.child_pid);
        Ok(())
    }
}

fn contains_enter_raw(bytes: &[u8]) -> bool {
    const ENTER: &[u8] = b"\x1b[?1049h";
    bytes.windows(ENTER.len()).any(|w| w == ENTER)
}

fn contains_exit_raw(bytes: &[u8]) -> bool {
    const EXIT: &[u8] = b"\x1b[?1049l";
    bytes.windows(EXIT.len()).any(|w| w == EXIT)
}

fn render_stats_bar(m: &SessionMetrics) {
    let bar = format!(
        "[ptyx] RTT: {}ms  saved: {}B  acc: {:.0}%  flushes: {}",
        m.rtt_estimate().as_millis(),
        m.bytes_saved(),
        m.prediction_accuracy() * 100.0,
        m.total_flushes(),
    );
    let Ok((cols, rows)) = terminal::size() else {
        return;
    };
    let _ = execute!(
        std::io::stdout(),
        cursor::SavePosition,
        cursor::MoveTo(0, rows.saturating_sub(1)),
        terminal::Clear(terminal::ClearType::CurrentLine),
        style::Print(&bar[..bar.len().min(cols as usize)]),
        cursor::RestorePosition,
    );
}

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

    #[test]
    fn contains_enter_raw_detects_alt_screen_sequence() {
        assert!(contains_enter_raw(b"\x1b[?1049h"));
        assert!(contains_enter_raw(b"prefix\x1b[?1049hsuffix"));
        assert!(!contains_enter_raw(b"hello world"));
    }

    #[test]
    fn contains_exit_raw_detects_alt_screen_exit() {
        assert!(contains_exit_raw(b"\x1b[?1049l"));
        assert!(contains_exit_raw(b"prefix\x1b[?1049lsuffix"));
        assert!(!contains_exit_raw(b"hello world"));
    }
}
