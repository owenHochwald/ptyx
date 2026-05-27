use anyhow::Result;
use clap::Parser;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct BufferConfig {
    pub flush_interval_ms: u64,
    pub max_size: usize,
}

impl Default for BufferConfig {
    fn default() -> Self {
        Self {
            flush_interval_ms: 5,
            max_size: 512,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub target: String,
    pub extra_ssh_args: Vec<String>,
    pub buffer: BufferConfig,
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
            },
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
    #[arg(long = "buffer", short = 'b', default_value_t = 5)]
    buffer_ms: u64,

    /// Maximum buffer size in bytes before forced flush
    #[arg(long = "max-size", short = 's', default_value_t = 512)]
    max_size: usize,

    /// Extra arguments passed through to ssh
    #[arg(last = true)]
    ssh_args: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_flush_interval_is_5ms() {
        let cfg = BufferConfig::default();
        assert_eq!(cfg.flush_interval_ms, 5);
    }

    #[test]
    fn default_config_max_size_is_512() {
        let cfg = BufferConfig::default();
        assert_eq!(cfg.max_size, 512);
    }

    #[test]
    fn buffer_config_debug_impl() {
        let cfg = BufferConfig::default();
        assert!(format!("{:?}", cfg).len() > 0);
    }

    #[test]
    fn ssh_args_appends_target() {
        let cfg = Config {
            target: "user@host".into(),
            extra_ssh_args: vec!["-p".into(), "2222".into()],
            buffer: BufferConfig::default(),
        };
        let args = cfg.ssh_args();
        assert_eq!(args, vec!["-p", "2222", "user@host"]);
    }
}
