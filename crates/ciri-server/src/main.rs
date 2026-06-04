use anyhow::Result;

#[cfg(target_os = "linux")]
use std::ffi::CString;

mod daemon;
mod session;
mod shell_integration;
mod tray;

#[cfg(target_os = "linux")]
fn set_process_name(name: &str) {
    let Ok(name) = CString::new(name) else {
        return;
    };

    unsafe {
        libc::prctl(libc::PR_SET_NAME, name.as_ptr() as libc::c_ulong, 0, 0, 0);
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "--print-socket-path") {
        println!(
            "{}",
            ciri_protocol::transport::server_socket_path().display()
        );
        return Ok(());
    }

    let headless = args.iter().any(|a| a == "--headless");

    #[cfg(unix)]
    let daemonized = if args.iter().any(|a| a == "--daemonize") {
        daemonize()?;
        true
    } else {
        false
    };
    #[cfg(not(unix))]
    let daemonized = false;

    if !daemonized {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    }

    #[cfg(target_os = "linux")]
    set_process_name("ciritty");

    // Set environment variables BEFORE creating the tokio runtime, since
    // Runtime::new() spawns worker threads and std::env::set_var is unsound
    // in the presence of concurrent threads (Rust 2024 edition).
    // SAFETY: No other threads exist yet — we are still in single-threaded main().
    unsafe {
        std::env::set_var("TERM_PROGRAM", "ciritty");
        std::env::set_var("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION"));
        std::env::set_var("COLORTERM", "truecolor");
        std::env::set_var("LC_TERMINAL", "ciritty");
        std::env::set_var("LC_TERMINAL_VERSION", env!("CARGO_PKG_VERSION"));
    }

    // `--web[ -port|-bind|-token|-static-dir ]` overrides the loaded
    // `[web]` config in memory (used by `ciritty web`, which spawns this
    // binary). `None` when no such flag is present → config from disk.
    let web_override = parse_web_overrides(&args)?;

    // Daemonized or headless mode: run tokio on main thread (no tray).
    if daemonized || headless {
        let rt = tokio::runtime::Runtime::new()?;
        return rt.block_on(daemon::run_daemon_with(web_override));
    }

    // Tray mode: tokio runs on a background thread, main thread runs tray event loop.
    let rt = tokio::runtime::Runtime::new()?;

    // Prepare the server state and bind the socket.
    let ds = rt.block_on(daemon::prepare_daemon_with(web_override))?;
    let tray_state = ds.state.clone();
    let tray_shutdown = ds.shutdown.clone();
    let server_exited = ds.server_exited.clone();

    // Spawn the daemon accept loop on the tokio runtime.
    rt.spawn(async move {
        if let Err(e) = daemon::run_daemon_loop(ds).await {
            log::error!("daemon error: {e}");
        }
    });

    // Run the tray on the main thread (returns on Quit or external shutdown).
    tray::run_tray(tray_state, tray_shutdown, server_exited);

    // Wait for tokio tasks (graceful_shutdown) to finish before exiting.
    rt.shutdown_timeout(std::time::Duration::from_secs(10));
    Ok(())
}

/// Parse `--web` / `--web-port` / `--web-bind` / `--web-token` /
/// `--web-static-dir` into an in-memory config override. Returns `None`
/// when none are present (config is then loaded from disk as usual).
///
/// Any web flag implies `web.enabled = true`. The per-field web
/// invariants (token ≥ 16 bytes, parseable bind, Origin policy on a
/// non-loopback bind) are enforced at daemon startup, so this only
/// surfaces flag-shape errors (e.g. a non-numeric port).
fn parse_web_overrides(args: &[String]) -> Result<Option<ciri_config::config::CiriConfig>> {
    let has_web = args
        .iter()
        .any(|a| a == "--web" || a.starts_with("--web-"));
    if !has_web {
        return Ok(None);
    }

    let mut config = ciri_config::config::CiriConfig::load()?;
    config.web.enabled = true;

    let mut i = 0;
    while i < args.len() {
        let next = |i: usize, flag: &str| -> Result<String> {
            args.get(i + 1)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("{flag} requires a value"))
        };
        match args[i].as_str() {
            "--web" => {}
            "--web-port" => {
                let v = next(i, "--web-port")?;
                config.web.port = v
                    .parse()
                    .map_err(|_| anyhow::anyhow!("--web-port: {v:?} is not a valid port"))?;
                i += 1;
            }
            "--web-bind" => {
                config.web.bind = next(i, "--web-bind")?;
                i += 1;
            }
            "--web-token" => {
                config.web.token = next(i, "--web-token")?;
                i += 1;
            }
            "--web-static-dir" => {
                config.web.static_dir = next(i, "--web-static-dir")?;
                i += 1;
            }
            // A `--web*` arg that reached here is unrecognized: a typo
            // (`--web-prot`) or the unsupported equals-form
            // (`--web-port=7892`). `has_web` already accepted it and turned
            // the gateway on, so silently ignoring it would start the server
            // on unintended (saved/default) settings — reject it instead.
            other if other.starts_with("--web") => {
                anyhow::bail!(
                    "unknown web flag {other:?}; supported: --web, --web-port N, \
                     --web-bind ADDR, --web-token TOK, --web-static-dir DIR \
                     (pass each value space-separated, not --flag=value)"
                );
            }
            _ => {}
        }
        i += 1;
    }

    // Re-validate: schema checks already ran inside `CiriConfig::load`, but
    // the overrides above mutated `web.*` afterward. Without this, inputs a
    // TOML load would reject (`--web-port 0`, a placeholder `--web-token`,
    // a non-loopback bind with no allowed_origins) could reach runtime.
    config.validate_schema()?;

    Ok(Some(config))
}

/// Fork into a background daemon.
#[cfg(unix)]
fn daemonize() -> Result<()> {
    use std::fs::OpenOptions;
    use std::os::fd::IntoRawFd;

    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return Err(anyhow::anyhow!(
            "fork failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    if pid > 0 {
        std::process::exit(0);
    }

    if unsafe { libc::setsid() } == -1 {
        return Err(anyhow::anyhow!(
            "setsid failed: {}",
            std::io::Error::last_os_error()
        ));
    }

    let devnull = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/null")?;
    let fd = devnull.into_raw_fd();
    unsafe {
        if libc::dup2(fd, libc::STDIN_FILENO) == -1
            || libc::dup2(fd, libc::STDOUT_FILENO) == -1
            || libc::dup2(fd, libc::STDERR_FILENO) == -1
        {
            // Close fd to avoid leak on error path.
            if fd > libc::STDERR_FILENO {
                libc::close(fd);
            }
            return Err(anyhow::anyhow!(
                "dup2 failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        if fd > libc::STDERR_FILENO {
            libc::close(fd);
        }
    }

    let log_dir = ciri_protocol::transport::runtime_dir().join("ciri");
    std::fs::create_dir_all(&log_dir)?;
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join("server.log"))?;
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .target(env_logger::Target::Pipe(Box::new(log_file)))
        .init();

    Ok(())
}
