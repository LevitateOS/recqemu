//! Command execution over the serial console (see `protocol`).

use anyhow::{bail, Result};
use std::time::{Duration, Instant};

use super::ansi::strip_ansi_codes;
use super::console::{CommandResult, Console};
use super::protocol::CommandMarkers;

impl Console {
    /// Execute a command and capture its output and exit code.
    pub fn exec(&mut self, command: &str, timeout: Duration) -> Result<CommandResult> {
        let markers = CommandMarkers::new();
        self.send(&markers.wrap(command))?;

        let started = Instant::now();
        let mut output = String::new();
        let mut collecting = false;
        while started.elapsed() < timeout {
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
                let exit_code = status.trim().parse().unwrap_or(-1);
                return Ok(CommandResult {
                    completed: true,
                    exit_code,
                    output,
                });
            }
            if collecting {
                output.push_str(&line);
                output.push('\n');
            }
        }
        Ok(CommandResult {
            completed: false,
            exit_code: -1,
            output,
        })
    }

    /// Execute a command that's expected to succeed.
    pub fn exec_ok(&mut self, command: &str, timeout: Duration) -> Result<String> {
        let result = self.exec(command, timeout)?;
        if !result.success() {
            bail!(
                "Command failed (exit {}): {}\nOutput: {}",
                result.exit_code,
                command,
                result.output
            );
        }
        Ok(result.output)
    }
}
