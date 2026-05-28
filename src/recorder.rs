use anyhow::{Context, Result};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Logs all PTY session I/O to a file in `~/.local/share/ptyx/sessions/`.
///
/// Format: text lines — `PTYX v1 <unix_seconds>` header, then
/// `I <elapsed_us> <hex>` for stdin and `O <elapsed_us> <hex>` for stdout.
pub struct SessionRecorder {
    writer: BufWriter<std::fs::File>,
    start: Instant,
    path: PathBuf,
}

impl SessionRecorder {
    /// Create a new session log file and write the header line.
    pub fn new() -> Result<Self> {
        let dir = sessions_dir();
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("creating sessions dir: {}", dir.display()))?;

        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let path = dir.join(format!("{ts}.ptyx"));

        let file = std::fs::File::create(&path)
            .with_context(|| format!("creating session file: {}", path.display()))?;
        let mut writer = BufWriter::new(file);
        writeln!(writer, "PTYX v1 {ts}")?;

        Ok(Self {
            writer,
            start: Instant::now(),
            path,
        })
    }

    /// Record bytes typed at the keyboard (stdin → PTY master direction).
    pub fn record_input(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        let us = self.start.elapsed().as_micros();
        writeln!(self.writer, "I {us} {}", hex_encode(bytes))
    }

    /// Record bytes received from the remote host (PTY master → stdout direction).
    pub fn record_output(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        let us = self.start.elapsed().as_micros();
        writeln!(self.writer, "O {us} {}", hex_encode(bytes))
    }

    /// Path of the session log file.
    pub fn session_path(&self) -> &Path {
        &self.path
    }

    /// Flush the writer.
    pub fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }
}

impl std::fmt::Debug for SessionRecorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionRecorder")
            .field("path", &self.path)
            .finish()
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn sessions_dir() -> PathBuf {
    std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join(".local").join("share")
        })
        .join("ptyx")
        .join("sessions")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn temp_sessions_dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn make_recorder_in(dir: &Path) -> SessionRecorder {
        let path = dir.join("test.ptyx");
        let file = std::fs::File::create(&path).unwrap();
        let mut writer = BufWriter::new(file);
        let ts = 0u64;
        writeln!(writer, "PTYX v1 {ts}").unwrap();
        SessionRecorder {
            writer,
            start: Instant::now(),
            path,
        }
    }

    #[test]
    fn recorder_writes_header_line() {
        let dir = temp_sessions_dir();
        let mut rec = make_recorder_in(dir.path());
        rec.flush().unwrap();

        let mut content = String::new();
        std::fs::File::open(rec.session_path())
            .unwrap()
            .read_to_string(&mut content)
            .unwrap();
        assert!(
            content.starts_with("PTYX v1 "),
            "header not found in: {content:?}"
        );
    }

    #[test]
    fn recorder_records_input_event() {
        let dir = temp_sessions_dir();
        let mut rec = make_recorder_in(dir.path());
        rec.record_input(b"ls").unwrap();
        rec.flush().unwrap();

        let content = std::fs::read_to_string(rec.session_path()).unwrap();
        assert!(content.contains("I "), "no I event in: {content:?}");
        assert!(content.contains("6c73"), "hex for 'ls' not found");
    }

    #[test]
    fn recorder_records_output_event() {
        let dir = temp_sessions_dir();
        let mut rec = make_recorder_in(dir.path());
        rec.record_output(b"ok").unwrap();
        rec.flush().unwrap();

        let content = std::fs::read_to_string(rec.session_path()).unwrap();
        assert!(content.contains("O "), "no O event in: {content:?}");
        assert!(content.contains("6f6b"), "hex for 'ok' not found");
    }

    #[test]
    fn recorder_elapsed_appears_in_events() {
        let dir = temp_sessions_dir();
        let mut rec = make_recorder_in(dir.path());
        rec.record_input(b"a").unwrap();
        rec.record_output(b"a").unwrap();
        rec.flush().unwrap();

        let content = std::fs::read_to_string(rec.session_path()).unwrap();
        // Both events must have a numeric elapsed field
        let lines: Vec<&str> = content.lines().collect();
        for line in &lines[1..] {
            let parts: Vec<&str> = line.splitn(3, ' ').collect();
            assert_eq!(parts.len(), 3);
            parts[1].parse::<u128>().expect("elapsed should be numeric");
        }
    }

    #[test]
    fn recorder_skips_empty_input() {
        let dir = temp_sessions_dir();
        let mut rec = make_recorder_in(dir.path());
        rec.record_input(b"").unwrap();
        rec.flush().unwrap();

        let content = std::fs::read_to_string(rec.session_path()).unwrap();
        // Only the header line
        assert_eq!(content.lines().count(), 1);
    }

    #[test]
    fn recorder_session_path_is_a_file() {
        let dir = temp_sessions_dir();
        let rec = make_recorder_in(dir.path());
        assert!(rec.session_path().is_file());
    }

    #[test]
    fn hex_encode_known_bytes() {
        assert_eq!(hex_encode(b"ls"), "6c73");
        assert_eq!(hex_encode(&[0x00, 0xFF]), "00ff");
    }
}
