use std::io::{self, Write};

/// Writes predicted echo and raw output to stdout; handles misprediction correction.
///
/// Tracks how many visible columns of predicted text are outstanding so that
/// `correct()` can erase and overwrite them when the server disagrees.
#[derive(Debug, Default)]
pub struct Display {
    /// Visible terminal columns used by the current pending prediction.
    predicted_cols: usize,
}

impl Display {
    pub fn new() -> Self {
        Self::default()
    }

    /// Write predicted text to stdout and track its visible column count.
    pub fn write_predicted(&mut self, text: &str) -> io::Result<()> {
        self.predicted_cols += visible_cols(text.as_bytes());
        let mut out = io::stdout();
        out.write_all(text.as_bytes())?;
        out.flush()
    }

    /// Write raw bytes without prediction tracking.
    pub fn write_raw(&self, bytes: &[u8]) -> io::Result<()> {
        let mut out = io::stdout();
        out.write_all(bytes)?;
        out.flush()
    }

    /// Erase predicted text and write the actual correction in its place.
    ///
    /// Uses backspace-space-backspace to overwrite each predicted column, then
    /// writes the correction. Resets the predicted column counter.
    pub fn correct(&mut self, correction: &str) -> io::Result<()> {
        let mut out = io::stdout();
        // Erase each pending predicted column
        for _ in 0..self.predicted_cols {
            out.write_all(b"\x08 \x08")?;
        }
        self.predicted_cols = 0;
        out.write_all(correction.as_bytes())?;
        out.flush()
    }

    /// Discard the pending prediction counter; call when prediction is confirmed correct.
    pub fn clear_predicted(&mut self) {
        self.predicted_cols = 0;
    }
}

/// Count visible terminal columns advanced by the given bytes.
///
/// Recognises the backspace-erase sequence `\x08 \x08` produced by `EchoPredictor`
/// (counts as −1 col). Newlines reset the counter to 0 (cursor moved to a new line).
fn visible_cols(bytes: &[u8]) -> usize {
    let mut cols: isize = 0;
    let mut i = 0;
    while i < bytes.len() {
        // Backspace erase sequence: \x08 \x20 \x08 (3 bytes) → net −1 col
        if i + 2 < bytes.len() && bytes[i] == 0x08 && bytes[i + 1] == b' ' && bytes[i + 2] == 0x08 {
            cols = (cols - 1).max(0);
            i += 3;
        } else {
            match bytes[i] {
                b'\r' | b'\n' => cols = 0, // cursor moved to new line
                0x20..=0x7E => cols += 1,  // printable ASCII
                _ => {}
            }
            i += 1;
        }
    }
    cols as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_cols_printable_ascii() {
        assert_eq!(visible_cols(b"hello"), 5);
    }

    #[test]
    fn visible_cols_resets_on_newline() {
        assert_eq!(visible_cols(b"abc\r\nxy"), 2);
    }

    #[test]
    fn visible_cols_backspace_erase_decrements() {
        // predict 'a' then backspace over it: net 0 cols
        let erase = b"\x08 \x08";
        assert_eq!(visible_cols(erase), 0);
    }

    #[test]
    fn visible_cols_mixed() {
        // "ab" then erase one → net 1 col
        let s = b"ab\x08 \x08";
        assert_eq!(visible_cols(s), 1);
    }
}
