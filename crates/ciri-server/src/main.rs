use anyhow::Result;

mod daemon;
mod session;

fn main() -> Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info"),
    )
    .init();

    let args: Vec<String> = std::env::args().collect();
    let session_name = args.get(1).cloned().unwrap_or_else(|| "default".to_string());

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(daemon::run_daemon(&session_name))
}
