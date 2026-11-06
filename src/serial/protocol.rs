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
/// Marker text is `{PREFIX}{name}_{id}{SUFFIX}`; the guest prints it with
/// printf, so the echoed input line holds `%s` where the output holds the id.
const PREFIX: &str = "___";
const SUFFIX: &str = "___";

fn marker(name: &str, id: &str) -> String {
    format!("{PREFIX}{name}_{id}{SUFFIX}")
}

/// Input that prints `marker(name, arg)` followed by `rest`.
fn print_marker(name: &str, arg: &str, rest: &str) -> String {
    format!("printf '{PREFIX}{name}_%s{SUFFIX}{rest}\\n' {arg}")
}

pub(crate) struct CommandMarkers {
    pub start: String,
    pub done: String,
    id: u64,
}

impl CommandMarkers {
    pub fn new() -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        Self {
            start: marker("START", &id.to_string()),
            done: marker("DONE", &id.to_string()),
            id,
        }
    }

    /// One input line: start marker, the command (quoted for `eval`, so a
    /// trailing comment cannot swallow the rest), then the done marker
    /// followed by the command's exit status. One line, so an echoing
    /// terminal shows it before the output, not inside it.
    pub fn wrap(&self, command: &str) -> String {
        let id = self.id.to_string();
        let quoted = format!("'{}'", command.replace('\'', "'\\''"));
        let start = print_marker("START", &id, "");
        let done = print_marker("DONE", &id, " %s");
        format!("{start}; eval {quoted}; {done} $?\n")
    }
}

/// Printed by the guest shell once a login succeeded.
pub(crate) fn login_ok_marker() -> String {
    marker("LOGIN", "OK")
}

/// Input that prints [`login_ok_marker`] (and nothing an echo would match).
pub(crate) fn login_probe() -> String {
    print_marker("LOGIN", "OK", "") + "\n"
}
