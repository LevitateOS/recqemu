//! Command execution over the serial console (see `protocol`).

use anyhow::{bail, Context, Result};
use std::time::{Duration, Instant};

use super::ansi::strip_ansi_codes;
use super::console::Console;
use super::protocol::CommandMarkers;
use crate::guest::{Guest, Outcome};

/// A serial input line stays below the terminal's canonical line bound
/// (4096) with room for the markers `CommandMarkers::wrap` adds.
const MAX_COMMAND_LEN: usize = 3000;

impl Console {
    /// Read until the done marker of a command that ran out of time, so its
    /// late output cannot land in the next command's.
    fn drain_pending(&mut self, budget: Duration) -> Result<()> {
        let Some(done) = self.pending_done.take() else {
            return Ok(());
        };
        let started = Instant::now();
        while started.elapsed() < budget {
            if let Some(line) = self.recv_line(Duration::from_millis(100))? {
                if strip_ansi_codes(&line).contains(&done) {
                    return Ok(());
                }
            }
        }
        self.pending_done = Some(done);
        bail!("the previous serial command is still running after its budget and {}s more", budget.as_secs())
    }
}

impl Guest for Console {
    /// Run a command in the guest shell and capture its output and exit code.
    fn exec(&mut self, command: &str, budget: Duration) -> Result<Outcome> {
        self.drain_pending(budget)?;
        let markers = CommandMarkers::new();
        self.send(&markers.wrap(command))?;

        let started = Instant::now();
        let mut output = String::new();
        let mut collecting = false;
        while started.elapsed() < budget {
            let Some(line) = self.recv_line(Duration::from_millis(100))? else {
                continue;
            };
            let clean = strip_ansi_codes(&line);
            let trimmed = clean.trim();

            if trimmed.contains(&markers.start) {
                collecting = true;
                continue;
            }
            if let Some((_, status)) = trimmed.split_once(&markers.done) {
                let code = status
                    .trim()
                    .parse()
                    .with_context(|| format!("the guest printed an unreadable exit status '{}'", status.trim()))?;
                return Ok(Outcome::Exited {
                    code,
                    output,
                    stderr: String::new(),
                });
            }
            if collecting {
                output.push_str(&clean);
                output.push('\n');
            }
        }
        self.pending_done = Some(markers.done);
        Ok(Outcome::TimedOut { partial: output })
    }

    fn max_command_len(&self) -> usize {
        MAX_COMMAND_LEN
    }
}
