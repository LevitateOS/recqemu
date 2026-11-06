//! The serial test protocol between recqemu and the guest shell.
//!
//! The guest side is `guest/serial-test.sh` ([`guest_profile`]), which
//! variants install as a profile.d script; the host side is [`Console`]'s
//! exec and login. Command markers are printed with `printf` arguments, so
//! an echoed command line never contains the marker it prints.
//!
//! [`Console`]: super::Console

use std::sync::atomic::{AtomicU64, Ordering};

/// Printed by the guest when an interactive serial shell takes commands.
pub const SHELL_READY_MARKER: &str = "___SHELL_READY___";

/// The guest profile script, with the readiness marker filled in.
pub fn guest_profile() -> String {
    include_str!("../../guest/serial-test.sh").replace("@SHELL_READY_MARKER@", SHELL_READY_MARKER)
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Markers around one command's output; unique within this process.
pub(crate) struct CommandMarkers {
    pub start: String,
    pub done: String,
    id: u64,
}

impl CommandMarkers {
    pub fn new() -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        Self {
            start: format!("___START_{id}___"),
            done: format!("___DONE_{id}___"),
            id,
        }
    }

    /// One input line: start marker, the command (quoted for `eval`, so a
    /// trailing comment cannot swallow the rest), then the done marker
    /// followed by the command's exit status. One line, so an echoing
    /// terminal shows it before the output, not inside it.
    pub fn wrap(&self, command: &str) -> String {
        let id = self.id;
        let quoted = format!("'{}'", command.replace('\'', "'\\''"));
        format!(
            "printf '___START_%s___\\n' {id}; eval {quoted}; printf '___DONE_%s___ %s\\n' {id} $?\n"
        )
    }
}

/// Printed by the guest shell once a login succeeded.
pub(crate) const LOGIN_OK_MARKER: &str = "___LOGIN_OK___";

/// Input that prints [`LOGIN_OK_MARKER`] (and nothing an echo would match).
pub(crate) const LOGIN_PROBE: &str = "printf '___LOGIN_%s___\\n' OK\n";
