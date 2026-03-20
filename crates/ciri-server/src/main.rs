use anyhow::Result;

mod daemon;
mod session;

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(daemon::run_daemon())
}
