use anyhow::Result;
use ptyx::{config::RunMode, proxy::PtyProxy, replay};

fn main() -> Result<()> {
    match RunMode::parse()? {
        RunMode::Proxy(config) => {
            if config.verbose && std::env::var("RUST_LOG").is_err() {
                unsafe { std::env::set_var("RUST_LOG", "ptyx=debug") };
            }
            tracing_subscriber::fmt()
                .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
                .init();
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(async move { PtyProxy::new(config)?.run().await })
        }
        RunMode::Replay(path) => {
            tracing_subscriber::fmt().init();
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(replay::replay_session(&path))
        }
    }
}
