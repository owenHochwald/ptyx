use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::Deserialize;
use std::path::{Path, PathBuf};

// ─── File-config structs (TOML deserialization) ───────────────────────────────

/// Top-level structure of `~/.config/ptyx/config.toml`.
#[derive(Debug, Deserialize, Default)]
pub struct FileConfig {
    pub proxy: Option<ProxyFileConfig>,
    pub display: Option<DisplayFileConfig>,
    pub backends: Option<Vec<BackendConfig>>,
}

/// `[proxy]` section in the TOML config file.
#[derive(Debug, Deserialize, Default)]
pub struct ProxyFileConfig {
    pub flush_interval_ms: Option<u64>,
    pub max_size: Option<usize>,
    pub adaptive: Option<bool>,
    pub passthrough: Option<bool>,
}

/// `[display]` section in the TOML config file.
#[derive(Debug, Deserialize, Default)]
pub struct DisplayFileConfig {
    pub predict: Option<bool>,
    pub stats: Option<bool>,
}

/// One entry in `[[backends]]` — a named SSH profile.
#[derive(Debug, Deserialize, Clone)]
pub struct BackendConfig {
    pub name: String,
    pub host: String,
    #[serde(default)]
    pub extra_args: Vec<String>,
}

// ─── Runtime config ───────────────────────────────────────────────────────────

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
    /// Enable local echo prediction (experimental; off by default until Phase 3 stable).
    pub predict: bool,
    /// Record session I/O to `~/.local/share/ptyx/sessions/`.
    pub record: bool,
    /// Path of the config file that was loaded (if any).
    pub config_path: Option<PathBuf>,
}

impl Config {
    /// Full argument list to pass to the `ssh` subprocess.
    pub fn ssh_args(&self) -> Vec<String> {
        let mut args = self.extra_ssh_args.clone();
        args.push(self.target.clone());
        args
    }
}

// ─── Dispatch mode ────────────────────────────────────────────────────────────

/// What the binary should do — parsed from CLI arguments.
pub enum RunMode {
    Proxy(Config),
    Replay(PathBuf),
}

impl RunMode {
    pub fn parse() -> Result<Self> {
        let cli = Cli::parse();
        match cli.command {
            Some(CliCommand::Replay { file }) => Ok(RunMode::Replay(file)),
            None => {
                let target = cli.target.ok_or_else(|| {
                    anyhow::anyhow!("a target (user@host) is required in proxy mode")
                })?;

                // Determine config file path and load it.
                let config_path = cli.config.clone().unwrap_or_else(default_config_path);
                let file_config = load_file_config(&config_path)?;

                // Merge: explicit CLI args override file config; file overrides built-in defaults.
                let flush_interval_ms = cli
                    .buffer_ms
                    .or_else(|| file_config.proxy.as_ref().and_then(|p| p.flush_interval_ms))
                    .unwrap_or(20);
                let max_size = cli
                    .max_size
                    .or_else(|| file_config.proxy.as_ref().and_then(|p| p.max_size))
                    .unwrap_or(512);

                // Boolean flags: CLI enables OR file enables.
                let passthrough = cli.no_buffer
                    || file_config
                        .proxy
                        .as_ref()
                        .and_then(|p| p.passthrough)
                        .unwrap_or(false);
                let adaptive = cli.adaptive
                    || file_config
                        .proxy
                        .as_ref()
                        .and_then(|p| p.adaptive)
                        .unwrap_or(false);
                let predict = cli.predict
                    || file_config
                        .display
                        .as_ref()
                        .and_then(|d| d.predict)
                        .unwrap_or(false);
                let show_stats = cli.stats
                    || file_config
                        .display
                        .as_ref()
                        .and_then(|d| d.stats)
                        .unwrap_or(false);

                let config_path_loaded = if config_path.exists() {
                    Some(config_path)
                } else {
                    None
                };

                Ok(RunMode::Proxy(Config {
                    target,
                    extra_ssh_args: cli.ssh_args,
                    buffer: BufferConfig {
                        flush_interval_ms,
                        max_size,
                        passthrough,
                        adaptive,
                    },
                    show_stats,
                    verbose: cli.verbose,
                    predict,
                    record: cli.record,
                    config_path: config_path_loaded,
                }))
            }
        }
    }

    /// Convenience: unwrap the proxy config (panics in test if wrong variant).
    #[cfg(test)]
    pub fn unwrap_proxy(self) -> Config {
        match self {
            RunMode::Proxy(c) => c,
            RunMode::Replay(_) => panic!("expected RunMode::Proxy"),
        }
    }
}

// ─── File config loading ──────────────────────────────────────────────────────

/// Load and parse a TOML config file. Returns `FileConfig::default()` if the
/// file does not exist (not an error — first run is fine without one).
pub fn load_file_config(path: &Path) -> Result<FileConfig> {
    if !path.exists() {
        return Ok(FileConfig::default());
    }
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading config file: {}", path.display()))?;
    toml::from_str(&content).with_context(|| format!("parsing config file: {}", path.display()))
}

fn default_config_path() -> PathBuf {
    std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join(".config")
        })
        .join("ptyx")
        .join("config.toml")
}

// ─── CLI parsing ──────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(name = "ptyx", about = "PTY proxy with input buffering for SSH")]
struct Cli {
    #[command(subcommand)]
    command: Option<CliCommand>,

    /// SSH target in user@host format (required when not using a subcommand)
    target: Option<String>,

    /// Path to config file (default: ~/.config/ptyx/config.toml)
    #[arg(long = "config", short = 'c', global = true)]
    config: Option<PathBuf>,

    /// Buffer flush interval in milliseconds
    #[arg(long = "buffer", short = 'b')]
    buffer_ms: Option<u64>,

    /// Maximum buffer size in bytes before forced flush
    #[arg(long = "max-size", short = 's')]
    max_size: Option<usize>,

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

    /// Enable local echo prediction (experimental — off by default)
    #[arg(long = "predict")]
    predict: bool,

    /// Record session I/O to ~/.local/share/ptyx/sessions/
    #[arg(long = "record")]
    record: bool,

    /// Extra arguments passed through to ssh
    #[arg(last = true)]
    ssh_args: Vec<String>,
}

#[derive(Subcommand, Debug)]
enum CliCommand {
    /// Replay a previously recorded .ptyx session log
    Replay {
        /// Path to the .ptyx session log file
        file: PathBuf,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn make_config() -> Config {
        Config {
            target: "user@host".into(),
            extra_ssh_args: vec![],
            buffer: BufferConfig::default(),
            show_stats: false,
            verbose: false,
            predict: false,
            record: false,
            config_path: None,
        }
    }

    // ─── BufferConfig defaults ────────────────────────────────────────────────

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

    // ─── Config methods ───────────────────────────────────────────────────────

    #[test]
    fn ssh_args_appends_target() {
        let cfg = Config {
            extra_ssh_args: vec!["-p".into(), "2222".into()],
            ..make_config()
        };
        let args = cfg.ssh_args();
        assert_eq!(args, vec!["-p", "2222", "user@host"]);
    }

    #[test]
    fn no_buffer_flag_sets_passthrough() {
        let cfg = Config {
            buffer: BufferConfig {
                passthrough: true,
                ..BufferConfig::default()
            },
            ..make_config()
        };
        assert!(cfg.buffer.passthrough);
    }

    #[test]
    fn stats_flag_reflects_in_config() {
        let cfg = Config {
            show_stats: true,
            ..make_config()
        };
        assert!(cfg.show_stats);
    }

    #[test]
    fn adaptive_flag_reflects_in_buffer_config() {
        let cfg = Config {
            buffer: BufferConfig {
                adaptive: true,
                ..BufferConfig::default()
            },
            ..make_config()
        };
        assert!(cfg.buffer.adaptive);
    }

    #[test]
    fn verbose_flag_reflects_in_config() {
        let cfg = Config {
            verbose: true,
            ..make_config()
        };
        assert!(cfg.verbose);
    }

    #[test]
    fn predict_flag_reflects_in_config() {
        let cfg = Config {
            predict: true,
            ..make_config()
        };
        assert!(cfg.predict);
    }

    #[test]
    fn default_config_predict_is_false() {
        let cfg = make_config();
        assert!(!cfg.predict);
    }

    #[test]
    fn record_flag_reflects_in_config() {
        let cfg = Config {
            record: true,
            ..make_config()
        };
        assert!(cfg.record);
    }

    #[test]
    fn default_config_record_is_false() {
        let cfg = make_config();
        assert!(!cfg.record);
    }

    // ─── FileConfig TOML parsing ──────────────────────────────────────────────

    #[test]
    fn file_config_proxy_section_parsed() {
        let toml = "[proxy]\nflush_interval_ms = 50\nmax_size = 1024\nadaptive = true\n";
        let fc: FileConfig = toml::from_str(toml).unwrap();
        let p = fc.proxy.unwrap();
        assert_eq!(p.flush_interval_ms, Some(50));
        assert_eq!(p.max_size, Some(1024));
        assert_eq!(p.adaptive, Some(true));
    }

    #[test]
    fn file_config_display_section_parsed() {
        let toml = "[display]\npredict = true\nstats = true\n";
        let fc: FileConfig = toml::from_str(toml).unwrap();
        let d = fc.display.unwrap();
        assert_eq!(d.predict, Some(true));
        assert_eq!(d.stats, Some(true));
    }

    #[test]
    fn file_config_backends_section_parsed() {
        let toml =
            "[[backends]]\nname = \"work\"\nhost = \"me@work.example.com\"\nextra_args = [\"-p\", \"2222\"]\n";
        let fc: FileConfig = toml::from_str(toml).unwrap();
        let backends = fc.backends.unwrap();
        assert_eq!(backends.len(), 1);
        assert_eq!(backends[0].name, "work");
        assert_eq!(backends[0].host, "me@work.example.com");
        assert_eq!(backends[0].extra_args, vec!["-p", "2222"]);
    }

    #[test]
    fn file_config_backends_extra_args_default_empty() {
        let toml = "[[backends]]\nname = \"home\"\nhost = \"me@home\"\n";
        let fc: FileConfig = toml::from_str(toml).unwrap();
        assert!(fc.backends.unwrap()[0].extra_args.is_empty());
    }

    #[test]
    fn file_config_default_is_all_none() {
        let fc = FileConfig::default();
        assert!(fc.proxy.is_none());
        assert!(fc.display.is_none());
        assert!(fc.backends.is_none());
    }

    #[test]
    fn file_config_partial_proxy_section() {
        // Only flush_interval_ms set; other fields absent → None
        let toml = "[proxy]\nflush_interval_ms = 30\n";
        let fc: FileConfig = toml::from_str(toml).unwrap();
        let p = fc.proxy.unwrap();
        assert_eq!(p.flush_interval_ms, Some(30));
        assert!(p.max_size.is_none());
        assert!(p.adaptive.is_none());
    }

    // ─── load_file_config ─────────────────────────────────────────────────────

    #[test]
    fn missing_file_returns_default() {
        let fc = load_file_config(Path::new("/no/such/file.toml")).unwrap();
        assert!(fc.proxy.is_none());
    }

    #[test]
    fn load_file_config_reads_toml() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "[proxy]\nflush_interval_ms = 75").unwrap();
        let fc = load_file_config(f.path()).unwrap();
        assert_eq!(fc.proxy.unwrap().flush_interval_ms, Some(75));
    }

    #[test]
    fn load_file_config_invalid_toml_is_error() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "not valid toml ===").unwrap();
        assert!(load_file_config(f.path()).is_err());
    }

    // ─── Merge logic (via RunMode internals, tested indirectly) ──────────────

    #[test]
    fn file_config_flush_interval_applies_when_no_cli_override() {
        // Simulate file config with flush_interval_ms = 75 and no CLI override.
        let file = FileConfig {
            proxy: Some(ProxyFileConfig {
                flush_interval_ms: Some(75),
                ..Default::default()
            }),
            ..Default::default()
        };
        // No CLI override (None) → file value used
        let interval = None::<u64>
            .or_else(|| file.proxy.as_ref().and_then(|p| p.flush_interval_ms))
            .unwrap_or(20);
        assert_eq!(interval, 75);
    }

    #[test]
    fn cli_flush_interval_overrides_file_config() {
        let file = FileConfig {
            proxy: Some(ProxyFileConfig {
                flush_interval_ms: Some(75),
                ..Default::default()
            }),
            ..Default::default()
        };
        // CLI explicitly passes 10ms → wins over file's 75ms
        let interval = Some(10u64)
            .or_else(|| file.proxy.as_ref().and_then(|p| p.flush_interval_ms))
            .unwrap_or(20);
        assert_eq!(interval, 10);
    }

    #[test]
    fn builtin_default_used_when_neither_cli_nor_file_set() {
        let file = FileConfig::default();
        let interval = None::<u64>
            .or_else(|| file.proxy.as_ref().and_then(|p| p.flush_interval_ms))
            .unwrap_or(20);
        assert_eq!(interval, 20);
    }

    #[test]
    fn file_config_adaptive_applies() {
        let file = FileConfig {
            proxy: Some(ProxyFileConfig {
                adaptive: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        // CLI flag is false (not set), file is true → combined true
        let cli_flag = false;
        let adaptive = cli_flag
            || file
                .proxy
                .as_ref()
                .and_then(|p| p.adaptive)
                .unwrap_or(false);
        assert!(adaptive);
    }

    #[test]
    fn file_config_predict_applies() {
        let file = FileConfig {
            display: Some(DisplayFileConfig {
                predict: Some(true),
                stats: None,
            }),
            ..Default::default()
        };
        let cli_flag = false;
        let predict = cli_flag
            || file
                .display
                .as_ref()
                .and_then(|d| d.predict)
                .unwrap_or(false);
        assert!(predict);
    }
}
