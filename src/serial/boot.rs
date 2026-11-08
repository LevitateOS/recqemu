//! Waiting for a guest to boot, judged by console lines.

use anyhow::{bail, Result};
use std::time::{Duration, Instant};

use super::ansi::strip_ansi_codes;
use super::console::Console;

/// What boot output means; the patterns come from the caller.
pub struct BootPatterns<'a> {
    /// A line containing one means the boot is done (a readiness marker,
    /// a login prompt).
    pub success: &'a [String],
    /// A line containing one means the boot failed.
    pub fatal: &'a [String],
}

impl Console {
    /// Wait for boot until a success pattern appears, failing on a fatal
    /// pattern or when the console stays silent for `stall_timeout` (boot may
    /// take as long as it keeps printing).
    pub fn wait_for_boot(&mut self, stall_timeout: Duration, patterns: &BootPatterns<'_>) -> Result<()> {
        let mut last_output = Instant::now();
        let mut last_nudge = Instant::now();
        let mut kernel_started = false;
        loop {
            // Once the kernel runs (not at the firmware or boot menu), nudge
            // the console: getty prompts on a headless serial console appear
            // only after input, and an empty line is harmless to a shell.
            if kernel_started && last_nudge.elapsed() >= Duration::from_secs(3) {
                self.send("\n")?;
                last_nudge = Instant::now();
            }
            if last_output.elapsed() > stall_timeout {
                bail!(
                    "BOOT STALLED: no console output for {} seconds\n\nLast output:\n{}",
                    stall_timeout.as_secs(),
                    self.tail()
                );
            }
            let Some(line) = self.recv_line(Duration::from_millis(500))? else {
                continue;
            };
            last_output = Instant::now();
            let line = strip_ansi_codes(&line);
            kernel_started |= line.contains("Linux version");
            let found = |list: &[String]| list.iter().find(|p| line.contains(p.as_str())).cloned();
            if let Some(pattern) = found(patterns.fatal) {
                bail!("Boot failed: {pattern}\n\nContext:\n{}", self.tail());
            }
            if found(patterns.success).is_some() {
                return Ok(());
            }
        }
    }
}
