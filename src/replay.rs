use anyhow::{bail, Context, Result};
use std::path::Path;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

/// Direction of a recorded session event.
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Direction {
    Input,
    Output,
}

/// A single recorded I/O event from a `.ptyx` session log.
#[derive(Debug, PartialEq)]
pub struct SessionEvent {
    pub direction: Direction,
    /// Microseconds since session start when the event was recorded.
    pub elapsed_us: u128,
    pub data: Vec<u8>,
}

/// Parse a `.ptyx` session log into a list of events.
///
/// Returns an error if the file header is invalid; silently skips malformed
/// event lines so that truncated recordings can still be partially replayed.
pub fn parse_session(path: &Path) -> Result<Vec<SessionEvent>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading session file: {}", path.display()))?;

    let mut lines = content.lines();

    // Validate header
    let header = lines.next().unwrap_or("");
    if !header.starts_with("PTYX v1 ") {
        bail!(
            "invalid session file: expected 'PTYX v1 <ts>' header, got {:?}",
            header
        );
    }

    let mut events = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some(event) = parse_event_line(line) {
            events.push(event);
        } else {
            tracing::warn!(line = %line, "skipping malformed session event line");
        }
    }
    Ok(events)
}

fn parse_event_line(line: &str) -> Option<SessionEvent> {
    let mut parts = line.splitn(3, ' ');
    let dir_char = parts.next()?;
    let elapsed_str = parts.next()?;
    let hex_str = parts.next()?;

    let direction = match dir_char {
        "I" => Direction::Input,
        "O" => Direction::Output,
        _ => return None,
    };

    let elapsed_us = elapsed_str.parse::<u128>().ok()?;
    let data = hex_decode(hex_str)?;

    Some(SessionEvent {
        direction,
        elapsed_us,
        data,
    })
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    s.as_bytes()
        .chunks(2)
        .map(|chunk| {
            let hi = hex_nibble(chunk[0])?;
            let lo = hex_nibble(chunk[1])?;
            Some((hi << 4) | lo)
        })
        .collect()
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Replay a `.ptyx` session log to stdout, preserving inter-event timing.
pub async fn replay_session(path: &Path) -> Result<()> {
    let events = parse_session(path)?;
    let mut stdout = tokio::io::stdout();
    let mut last_elapsed_us: u128 = 0;

    for event in &events {
        // Sleep for the gap between events (capped at 2s to keep replays snappy).
        let gap_us = event.elapsed_us.saturating_sub(last_elapsed_us);
        let gap = Duration::from_micros(gap_us.min(2_000_000) as u64);
        if !gap.is_zero() {
            tokio::time::sleep(gap).await;
        }
        last_elapsed_us = event.elapsed_us;

        // Only replay output (O) events — stdin input is not re-sent.
        if event.direction == Direction::Output {
            stdout.write_all(&event.data).await?;
            stdout.flush().await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_session(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f
    }

    #[test]
    fn parse_header_valid() {
        let f = write_session("PTYX v1 1700000000\n");
        let events = parse_session(f.path()).unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn parse_rejects_bad_header() {
        let f = write_session("NOT A PTYX FILE\n");
        assert!(parse_session(f.path()).is_err());
    }

    #[test]
    fn parse_input_event() {
        let f = write_session("PTYX v1 0\nI 0 6c73\n");
        let events = parse_session(f.path()).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].direction, Direction::Input);
        assert_eq!(events[0].elapsed_us, 0);
        assert_eq!(events[0].data, b"ls");
    }

    #[test]
    fn parse_output_event() {
        let f = write_session("PTYX v1 0\nO 5432 6c730a\n");
        let events = parse_session(f.path()).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].direction, Direction::Output);
        assert_eq!(events[0].elapsed_us, 5432);
        assert_eq!(events[0].data, b"ls\n");
    }

    #[test]
    fn parse_mixed_events() {
        let f = write_session("PTYX v1 0\nI 0 61\nO 1000 6200\n");
        let events = parse_session(f.path()).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].direction, Direction::Input);
        assert_eq!(events[1].direction, Direction::Output);
        assert_eq!(events[1].elapsed_us, 1000);
    }

    #[test]
    fn parse_skips_malformed_lines() {
        // Line with unknown direction character — skipped, no error
        let f = write_session("PTYX v1 0\nX 0 61\nO 0 62\n");
        let events = parse_session(f.path()).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].direction, Direction::Output);
    }

    #[test]
    fn parse_empty_body() {
        let f = write_session("PTYX v1 0\n");
        let events = parse_session(f.path()).unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn hex_decode_roundtrips() {
        assert_eq!(hex_decode("6c73"), Some(b"ls".to_vec()));
        assert_eq!(hex_decode("00ff"), Some(vec![0x00, 0xFF]));
        assert_eq!(hex_decode(""), Some(vec![]));
    }

    #[test]
    fn hex_decode_rejects_odd_length() {
        assert_eq!(hex_decode("abc"), None);
    }

    #[test]
    fn hex_decode_rejects_non_hex() {
        assert_eq!(hex_decode("zz"), None);
    }
}
