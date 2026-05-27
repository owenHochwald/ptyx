use anyhow::Result;
use crossterm::terminal;

/// RAII guard for raw mode. Restores terminal on drop and on panic.
#[derive(Debug)]
pub struct Terminal {
    _raw: (),
}

impl Terminal {
    /// Enable raw mode and install a panic hook that restores the terminal.
    pub fn enter() -> Result<Terminal> {
        let original = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = terminal::disable_raw_mode();
            original(info);
        }));
        terminal::enable_raw_mode()?;
        Ok(Terminal { _raw: () })
    }

    pub fn current_size() -> Result<(u16, u16)> {
        let (cols, rows) = terminal::size()?;
        Ok((rows, cols))
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        if let Err(e) = terminal::disable_raw_mode() {
            tracing::error!(err = %e, "failed to disable raw mode on drop");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_struct_is_send() {
        fn _assert_send<T: Send>() {}
        _assert_send::<Terminal>();
    }

    #[test]
    fn terminal_drop_impl_exists() {
        // We can't actually enable raw mode in tests without a real TTY,
        // but we can construct the struct directly and confirm drop runs.
        let t = Terminal { _raw: () };
        drop(t); // Drop runs disable_raw_mode(); error is logged, not panicked
    }
}
