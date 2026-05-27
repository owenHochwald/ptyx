use anyhow::Result;
use ptyx::{config::Config, proxy::PtyProxy};

fn main() -> Result<()> {
    let config = Config::load_from_args()?;
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async move { PtyProxy::new(config)?.run().await })
}
