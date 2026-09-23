//! Adopt the user's login-shell environment when launched from the GUI.
//!
//! macOS apps started from Finder / Dock / a LaunchAgent inherit launchd's
//! bare environment (`PATH=/usr/bin:/bin:/usr/sbin:/sbin`, no Homebrew, no
//! version managers). Pane shells fix themselves up by running as login
//! shells, but everything the daemon spawns directly — `run-command`,
//! restored agent commands (`sh -c "claude … || exec $SHELL"`), plugin
//! jobs — would still see the bare PATH. Like VS Code's "resolve shell
//! environment", run the user's shell once as an interactive login shell,
//! capture its environment, and merge it into ours before any thread or
//! child exists.

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const MARKER: &str = "__LOOM_LOGIN_ENV__";
/// A slow or hung rc file must not block the daemon from starting.
const TIMEOUT: Duration = Duration::from_secs(5);

/// Variables that describe the capturing shell or the terminal rather than
/// the user's environment, plus ones the daemon's own paths depend on
/// (`TMPDIR` locates the control socket).
const SKIP: &[&str] = &[
    "_",
    "PWD",
    "OLDPWD",
    "SHLVL",
    "TERM",
    "TERM_PROGRAM",
    "TERM_PROGRAM_VERSION",
    "COLORTERM",
    "LC_TERMINAL",
    "LC_TERMINAL_VERSION",
    "TMPDIR",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
];

/// Merge the login-shell environment into this process. Must run while the
/// process is still single-threaded (`set_var` is unsound otherwise).
pub(crate) fn import() {
    // A terminal-launched daemon (`cargo run`, `loomtty` from a shell)
    // already carries a full environment; launchd never sets TERM.
    if std::env::var_os("TERM").is_some() {
        return;
    }
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/bin/zsh".to_string());

    let started = Instant::now();
    let vars = match capture(&shell) {
        Ok(vars) => vars,
        Err(e) => {
            log::warn!("login env: capturing from {shell} failed: {e}");
            return;
        }
    };

    let mut imported = 0;
    for (key, value) in vars {
        if SKIP.contains(&key.as_str()) || key.starts_with("LOOM_") {
            continue;
        }
        // PATH is the point of the exercise; for everything else keep
        // whatever the launcher explicitly gave us.
        if key == "PATH" || std::env::var_os(&key).is_none() {
            // SAFETY: called from single-threaded `main` before the tokio
            // runtime or any other thread is created.
            unsafe { std::env::set_var(&key, &value) };
            imported += 1;
        }
    }
    log::info!(
        "login env: imported {imported} vars from {shell} in {:?}",
        started.elapsed()
    );
}

fn capture(shell: &str) -> anyhow::Result<Vec<(String, String)>> {
    // `-i` so rc files that only configure interactive shells (the usual
    // home for PATH tweaks in ~/.zshrc / config.fish) run too. Markers
    // fence off anything the rc files print. The syntax is valid in
    // sh/bash/zsh/fish alike.
    let script = format!("printf '%s' {MARKER}; /usr/bin/env -0; printf '%s' {MARKER}");
    let mut child = Command::new(shell)
        .args(["-i", "-l", "-c", &script])
        .env("LOOM_RESOLVING_LOGIN_ENV", "1")
        .current_dir(std::env::var_os("HOME").unwrap_or_else(|| "/".into()))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    // Read on a helper thread so a chatty rc can't fill the pipe and
    // deadlock the timeout loop. Joined before we return, so the process
    // is single-threaded again when the caller calls `set_var`.
    let mut stdout = child.stdout.take().expect("stdout is piped");
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });

    let deadline = Instant::now() + TIMEOUT;
    loop {
        if child.try_wait()?.is_some() {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            anyhow::bail!("timed out after {TIMEOUT:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let out = reader
        .join()
        .map_err(|_| anyhow::anyhow!("reader thread panicked"))?;
    parse(&String::from_utf8_lossy(&out))
}

fn parse(out: &str) -> anyhow::Result<Vec<(String, String)>> {
    let mut parts = out.split(MARKER);
    let body = match (parts.next(), parts.next(), parts.next()) {
        (Some(_), Some(body), Some(_)) => body,
        _ => anyhow::bail!("markers not found in shell output"),
    };
    Ok(body
        .split('\0')
        .filter_map(|entry| entry.split_once('='))
        .filter(|(key, _)| !key.is_empty())
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ignores_rc_noise_around_markers() {
        let out = format!(
            "Welcome!\n{MARKER}PATH=/opt/homebrew/bin:/usr/bin\0LANG=en_US.UTF-8\0MULTI=a\nb\0{MARKER}bye"
        );
        let vars = parse(&out).unwrap();
        assert_eq!(
            vars,
            vec![
                ("PATH".into(), "/opt/homebrew/bin:/usr/bin".into()),
                ("LANG".into(), "en_US.UTF-8".into()),
                ("MULTI".into(), "a\nb".into()),
            ]
        );
    }

    #[test]
    fn parse_rejects_output_without_markers() {
        assert!(parse("PATH=/usr/bin\0").is_err());
    }

    #[test]
    fn capture_reads_env_from_a_real_shell() {
        let vars = capture("/bin/sh").unwrap();
        assert!(vars.iter().any(|(k, _)| k == "LOOM_RESOLVING_LOGIN_ENV"));
    }
}
