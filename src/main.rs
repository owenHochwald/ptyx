use anyhow::Result;
use ptyx::{config::Config, proxy::PtyProxy};

fn main() -> Result<()> {
    let config = Config::load_from_args()?;

    // --verbose sets debug level if the env var is not already configured.
    if config.verbose && std::env::var("RUST_LOG").is_err() {
        unsafe { std::env::set_var("RUST_LOG", "ptyx=debug") };
    }

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async move { PtyProxy::new(config)?.run().await })
}
