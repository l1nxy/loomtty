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

    // Daemonized or headless mode: run tokio on main thread (no tray).
    if daemonized || headless {
        let rt = tokio::runtime::Runtime::new()?;
        return rt.block_on(daemon::run_daemon());
    }

    // Tray mode: tokio runs on a background thread, main thread runs tray event loop.
    let rt = tokio::runtime::Runtime::new()?;

    // Prepare the server state and bind the socket.
    let ds = rt.block_on(daemon::prepare_daemon())?;
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
