use anyhow::{Context, Result};
use nix::pty::{openpty, OpenptyResult};
use nix::sys::signal::{Signal, kill};
use nix::sys::wait::{WaitPidFlag, waitpid};
use nix::unistd::{ForkResult, Pid, close, dup2, execvp, fork, setsid};
use std::ffi::CString;
use std::os::fd::{AsRawFd, OwnedFd};

pub struct Pty {
    master: OwnedFd,
    child_pid: Pid,
}

impl Pty {
    pub fn spawn(cols: u16, rows: u16) -> Result<Self> {
        let OpenptyResult { master, slave } = openpty(None, None)
            .context("openpty failed")?;

        set_winsize(master.as_raw_fd(), cols, rows);

        match unsafe { fork() }.context("fork failed")? {
            ForkResult::Child => {
                drop(master);
                setsid().ok();
                unsafe { libc::ioctl(slave.as_raw_fd(), libc::TIOCSCTTY, 0) };
                dup2(slave.as_raw_fd(), 0).ok();
                dup2(slave.as_raw_fd(), 1).ok();
                dup2(slave.as_raw_fd(), 2).ok();
                if slave.as_raw_fd() > 2 {
                    close(slave.as_raw_fd()).ok();
                }

                // Close inherited fds (3..1024) to avoid leaking GPU/event fds
                for fd in 3..1024 {
                    let _ = close(fd);
                }

                let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
                let shell_c = CString::new(shell).unwrap();
                let _ = execvp(&shell_c, &[&shell_c]);
                std::process::exit(1);
            }
            ForkResult::Parent { child } => {
                drop(slave);
                Ok(Pty {
                    master,
                    child_pid: child,
                })
            }
        }
    }

    pub fn master_fd(&self) -> &OwnedFd {
        &self.master
    }

    pub fn child_pid(&self) -> Pid {
        self.child_pid
    }

    /// Check if child process has exited (non-blocking).
    pub fn try_wait(&self) -> bool {
        match waitpid(self.child_pid, Some(WaitPidFlag::WNOHANG)) {
            Ok(nix::sys::wait::WaitStatus::Exited(_, _))
            | Ok(nix::sys::wait::WaitStatus::Signaled(_, _, _)) => true,
            _ => false,
        }
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        set_winsize(self.master.as_raw_fd(), cols, rows);
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        // Only signal+reap if the child hasn't been reaped yet.
        // kill() returns ESRCH if the process doesn't exist — that's fine.
        if kill(self.child_pid, Signal::SIGHUP).is_ok() {
            let _ = waitpid(self.child_pid, Some(WaitPidFlag::WNOHANG));
        }
    }
}

fn set_winsize(fd: i32, cols: u16, rows: u16) {
    let ws = libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    unsafe { libc::ioctl(fd, libc::TIOCSWINSZ, &ws) };
}
