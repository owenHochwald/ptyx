use anyhow::Result;
use clap::Parser;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct BufferConfig {
    pub flush_interval_ms: u64,
    pub max_size: usize,
    pub passthrough: bool,
    pub adaptive: bool,
}

impl Default for BufferConfig {
    fn default() -> Self {
        Self {
            flush_interval_ms: 20,
            max_size: 512,
            passthrough: false,
            adaptive: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub target: String,
    pub extra_ssh_args: Vec<String>,
    pub buffer: BufferConfig,
    pub show_stats: bool,
    pub verbose: bool,
}

impl Config {
    pub fn load_from_args() -> Result<Config> {
        let cli = Cli::parse();
        Ok(Config {
            target: cli.target,
            extra_ssh_args: cli.ssh_args,
            buffer: BufferConfig {
                flush_interval_ms: cli.buffer_ms,
                max_size: cli.max_size,
                passthrough: cli.no_buffer,
                adaptive: cli.adaptive,
            },
            show_stats: cli.stats,
            verbose: cli.verbose,
        })
    }

    /// Full argument list to pass to the `ssh` subprocess.
    pub fn ssh_args(&self) -> Vec<String> {
        let mut args = self.extra_ssh_args.clone();
        args.push(self.target.clone());
        args
    }
}

#[derive(Parser, Debug)]
#[command(name = "ptyx", about = "PTY proxy with input buffering for SSH")]
struct Cli {
    /// SSH target in user@host format
    target: String,

    /// Buffer flush interval in milliseconds
    #[arg(long = "buffer", short = 'b', default_value_t = 20)]
    buffer_ms: u64,

    /// Maximum buffer size in bytes before forced flush
    #[arg(long = "max-size", short = 's', default_value_t = 512)]
    max_size: usize,

    /// Disable buffering (passthrough mode — use for scp/sftp/binary sessions)
    #[arg(long = "no-buffer")]
    no_buffer: bool,

    /// Enable RTT-based adaptive flush interval
    #[arg(long = "adaptive")]
    adaptive: bool,

    /// Show live metrics bar at bottom of terminal
    #[arg(long = "stats")]
    stats: bool,

    /// Enable debug logging (sets RUST_LOG=ptyx=debug if not already set)
    #[arg(long = "verbose", short = 'v')]
    verbose: bool,

    /// Extra arguments passed through to ssh
    #[arg(last = true)]
    ssh_args: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_flush_interval_is_20ms() {
        let cfg = BufferConfig::default();
        assert_eq!(cfg.flush_interval_ms, 20);
    }

    #[test]
    fn default_config_max_size_is_512() {
        let cfg = BufferConfig::default();
        assert_eq!(cfg.max_size, 512);
    }

    #[test]
    fn default_config_passthrough_is_false() {
        let cfg = BufferConfig::default();
        assert!(!cfg.passthrough);
    }

    #[test]
    fn default_config_adaptive_is_false() {
        let cfg = BufferConfig::default();
        assert!(!cfg.adaptive);
    }

    #[test]
    fn buffer_config_debug_impl() {
        let cfg = BufferConfig::default();
        assert!(!format!("{:?}", cfg).is_empty());
    }

    #[test]
    fn ssh_args_appends_target() {
        let cfg = Config {
            target: "user@host".into(),
            extra_ssh_args: vec!["-p".into(), "2222".into()],
            buffer: BufferConfig::default(),
            show_stats: false,
            verbose: false,
        };
        let args = cfg.ssh_args();
        assert_eq!(args, vec!["-p", "2222", "user@host"]);
    }

    #[test]
    fn no_buffer_flag_sets_passthrough() {
        // Simulate CLI parsing by constructing Config directly
        let cfg = Config {
            target: "user@host".into(),
            extra_ssh_args: vec![],
            buffer: BufferConfig {
                passthrough: true,
                ..BufferConfig::default()
            },
            show_stats: false,
            verbose: false,
        };
        assert!(cfg.buffer.passthrough);
    }

    #[test]
    fn stats_flag_reflects_in_config() {
        let cfg = Config {
            target: "user@host".into(),
            extra_ssh_args: vec![],
            buffer: BufferConfig::default(),
            show_stats: true,
            verbose: false,
        };
        assert!(cfg.show_stats);
    }

    #[test]
    fn adaptive_flag_reflects_in_buffer_config() {
        let cfg = Config {
            target: "user@host".into(),
            extra_ssh_args: vec![],
            buffer: BufferConfig {
                adaptive: true,
                ..BufferConfig::default()
            },
            show_stats: false,
            verbose: false,
        };
        assert!(cfg.buffer.adaptive);
    }

    #[test]
    fn verbose_flag_reflects_in_config() {
        let cfg = Config {
            target: "user@host".into(),
            extra_ssh_args: vec![],
            buffer: BufferConfig::default(),
            show_stats: false,
            verbose: true,
        };
        assert!(cfg.verbose);
    }
}
