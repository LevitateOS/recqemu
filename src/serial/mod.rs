//! Serial console backend for QEMU.
//!
//! - `Console` - serial I/O with a transcript
//! - `exec()` / `exec_ok()` - run commands with exit code capture
//! - `wait_for_boot()` - wait for boot, judged by the caller's patterns
//! - `login()` - serial getty login
//! - `protocol` - the markers and the guest profile script both sides share

mod ansi;
mod auth;
mod boot;
mod console;
mod exec;
pub mod protocol;

pub use ansi::strip_ansi_codes;
pub use boot::BootPatterns;
pub use console::{CommandResult, Console};
