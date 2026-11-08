//! Driving a QEMU guest through its serial console. `protocol` holds what the
//! guest side must agree with.

mod ansi;
mod auth;
mod boot;
mod console;
mod exec;
pub mod protocol;

pub use ansi::strip_ansi_codes;
pub use boot::BootPatterns;
pub use console::Console;
