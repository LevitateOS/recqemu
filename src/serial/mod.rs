//! Serial console backend for QEMU.
//!
//! - `Console` - serial I/O with a transcript
//! - `exec()` / `exec_ok()` - run commands with exit code capture
//! - `wait_for_boot_with_patterns()` - wait for boot with configurable patterns
//! - `login()` - serial getty login
//! - `protocol` - the markers and the guest profile script both sides share

mod ansi;
mod auth;
mod boot;
mod console;
mod exec;
pub mod protocol;

pub use ansi::strip_ansi_codes;
pub use console::{CommandResult, Console};
