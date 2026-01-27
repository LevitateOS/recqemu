//! Serial console backend for QEMU.
//!
//! This module provides serial I/O for command execution with exit code capture.
//!
//! # Overview
//!
//! - `Console` - Serial I/O with command execution and exit code capture
//! - `exec()` / `exec_ok()` - Run commands with exit code capture
//! - `exec_chroot()` - Run commands in chroot via recchroot
//! - `wait_for_boot_with_patterns()` - Wait for boot with configurable patterns
//! - `write_file()` - Write files via serial console
//! - `login()` - Authentication subsystem (login, shell markers)

mod ansi;
mod auth;
mod boot;
mod chroot;
mod console;
mod exec;
mod sync;
mod utils;

pub use console::{CommandResult, Console};
pub use sync::{generate_command_markers, is_marker_line};
