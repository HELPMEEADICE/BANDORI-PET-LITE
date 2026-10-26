//! Launches and supervises the settings process (spec §2.9).

use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use tracing::{info, warn};

/// Keeps the settings process alive, restarting it at most once every 3 s.
pub struct UiSupervisor {
    executable: PathBuf,
    pipe: String,
    child: Option<Child>,
    last_restart: Option<Instant>,
    restarts: u32,
    /// Set when the executable is missing so we only complain once.
    warned_missing: bool,
    /// `true` while a settings client is attached to the pipe.
    connected: bool,
}

impl UiSupervisor {
    /// Creates a supervisor for `executable` talking to `pipe`.
    pub fn new(executable: PathBuf, pipe: String) -> Self {
        Self {
            executable,
            pipe,
            child: None,
            last_restart: None,
            restarts: 0,
            warned_missing: false,
            connected: false,
        }
    }

    /// Pipe name the child should connect to.
    pub fn pipe(&self) -> &str {
        &self.pipe
    }

    /// Whether the child process is currently running.
    pub fn is_running(&mut self) -> bool {
        match self.child.as_mut() {
            Some(child) => match child.try_wait() {
                Ok(None) => true,
                Ok(Some(_)) | Err(_) => {
                    self.child = None;
                    false
                }
            },
            None => false,
        }
    }

    /// Marks the child as connected (so `ensure_running` does not spawn a second one).
    pub fn note_connected(&mut self) {
        self.connected = true;
    }

    /// Clears the connected flag when the pipe closes.
    pub fn note_disconnected(&mut self) {
        self.connected = false;
    }

    /// Starts the settings process if it is not running.
    pub fn ensure_running(&mut self) {
        if self.is_running() || self.connected {
            return;
        }
        if !self.executable.exists() {
            if !self.warned_missing {
                warn!(
                    path = %self.executable.display(),
                    "settings executable missing; build `bandori-ui` or run from the install directory"
                );
                self.warned_missing = true;
            }
            return;
        }
        match Command::new(&self.executable)
            .arg("--pipe")
            .arg(&self.pipe)
            .spawn()
        {
            Ok(child) => {
                info!(pid = child.id(), "settings process started");
                self.child = Some(child);
                self.last_restart = Some(Instant::now());
            }
            Err(error) => warn!(%error, "failed to start settings process"),
        }
    }

    /// Restarts the child at most once per 3 s, and only after it was running.
    pub fn tick(&mut self) {
        let was_running = self.child.is_some();
        if self.is_running() {
            return;
        }
        if !was_running {
            return;
        }
        let Some(last) = self.last_restart else {
            return;
        };
        if last.elapsed() < Duration::from_secs(3) {
            return;
        }
        if self.restarts >= 3 {
            warn!("settings process crashed repeatedly; not restarting again");
            return;
        }
        self.restarts += 1;
        info!(attempt = self.restarts, "restarting settings process");
        self.ensure_running();
    }

    /// Asks the child to exit.
    pub fn shutdown(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
        }
        self.child = None;
    }
}

