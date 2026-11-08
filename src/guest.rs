//! A shell in a guest, reached over the serial console ([`Console`]) or SSH
//! (`recshuttle::Remote`): one interface, so callers do not depend on the
//! transport, with honest outcomes (a command that ran out of time says so).
//!
//! [`Console`]: crate::serial::Console

use anyhow::Result;
use std::time::Duration;

/// How a command ended.
#[derive(Debug, Clone)]
pub enum Outcome {
    /// It exited with `code`. `output` is what it printed (on SSH, stdout
    /// only; `stderr` apart); ANSI escapes are stripped.
    Exited { code: i32, output: String, stderr: String },
    /// It was still running when its budget ran out.
    TimedOut { partial: String },
}

impl Outcome {
    pub fn success(&self) -> bool {
        matches!(self, Self::Exited { code: 0, .. })
    }

    /// What it printed (so far, for a command that timed out).
    pub fn output(&self) -> &str {
        match self {
            Self::Exited { output, .. } => output,
            Self::TimedOut { partial } => partial,
        }
    }

    /// One line for an error message: `exit N` or `timed out`.
    pub fn describe(&self) -> String {
        match self {
            Self::Exited { code, .. } => format!("exit {code}"),
            Self::TimedOut { .. } => "timed out".to_string(),
        }
    }
}

/// A shell that runs commands in the guest. A transport failure (the VM
/// stopped, SSH could not connect) is an `Err`, not an outcome.
pub trait Guest {
    /// Run `command` in the guest's shell, waiting at most `budget`.
    fn exec(&mut self, command: &str, budget: Duration) -> Result<Outcome>;

    /// The longest command, after quoting, one `exec` should carry.
    fn max_command_len(&self) -> usize;
}

/// Quote `value` as one word for a POSIX shell.
pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
