//! Serial console login (getty → login → shell).

use anyhow::{bail, Result};
use std::time::{Duration, Instant};

use super::ansi::strip_ansi_codes;
use super::console::Console;
use super::protocol::{LOGIN_OK_MARKER, LOGIN_PROBE};

/// Logins rejected before giving up.
const MAX_ATTEMPTS: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    WaitingForLoginPrompt,
    WaitingForPasswordPrompt,
    WaitingForShell,
}

impl Console {
    /// Log in on the serial getty and wait until the shell runs commands.
    ///
    /// Once the password is sent, a probe that prints a marker is typed
    /// whenever the console has been quiet for a second; the marker only
    /// appears when a shell ran it.
    pub fn login(&mut self, username: &str, password: &str, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        let mut state = State::WaitingForLoginPrompt;
        let mut rejected = 0u32;
        let mut quiet_since = Instant::now();

        self.send("\n")?;
        while Instant::now() < deadline {
            if state == State::WaitingForShell && quiet_since.elapsed() >= Duration::from_secs(1) {
                self.send(LOGIN_PROBE)?;
                quiet_since = Instant::now();
            }
            let Some(line) = self.recv_line(Duration::from_millis(200))? else {
                continue;
            };
            quiet_since = Instant::now();
            let lower = strip_ansi_codes(&line).to_lowercase();

            if state != State::WaitingForLoginPrompt
                && (lower.contains("login incorrect") || lower.contains("authentication failure"))
            {
                rejected += 1;
                if rejected >= MAX_ATTEMPTS {
                    bail!(
                        "login as {username} was rejected {rejected} times\n\nLast output:\n{}",
                        self.tail()
                    );
                }
                state = State::WaitingForLoginPrompt;
                continue;
            }
            match state {
                State::WaitingForLoginPrompt if lower.contains("login:") => {
                    self.send(&format!("{username}\n"))?;
                    state = State::WaitingForPasswordPrompt;
                }
                State::WaitingForPasswordPrompt if lower.contains("password") => {
                    self.send(&format!("{password}\n"))?;
                    state = State::WaitingForShell;
                }
                State::WaitingForShell if line.contains(LOGIN_OK_MARKER) => return Ok(()),
                _ => {}
            }
        }
        bail!(
            "login as {username} timed out after {timeout:?} ({state:?})\n\nLast output:\n{}",
            self.tail()
        )
    }
}
