use anyhow::Result;

mod daemon;
mod session;
mod shell_integration;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "--print-socket-path") {
        println!(
            "{}",
            ciri_protocol::transport::server_socket_path().display()
        );
        return Ok(());
    }

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

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(daemon::run_daemon())
}

/// Fork into a background daemon.
///
/// Standard Unix recipe (same as tmux):
/// 1. fork — parent exits
/// 2. setsid — detach from controlling terminal
/// 3. redirect stdio to /dev/null
/// 4. init logger to file
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
        // Parent exits, child continues.
        std::process::exit(0);
    }

    // New session leader — detach from terminal.
    if unsafe { libc::setsid() } == -1 {
        return Err(anyhow::anyhow!(
            "setsid failed: {}",
            std::io::Error::last_os_error()
        ));
    }

    // Redirect stdin/stdout/stderr → /dev/null
    let devnull = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/null")?;
    let fd = devnull.into_raw_fd(); // prevent Drop from closing the fd
    unsafe {
        if libc::dup2(fd, libc::STDIN_FILENO) == -1
            || libc::dup2(fd, libc::STDOUT_FILENO) == -1
            || libc::dup2(fd, libc::STDERR_FILENO) == -1
        {
            return Err(anyhow::anyhow!(
                "dup2 failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        if fd > libc::STDERR_FILENO {
            libc::close(fd);
        }
    }

    // File-based logging since stdio is gone.
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
