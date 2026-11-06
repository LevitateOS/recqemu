//! Waiting for a guest to boot, judged by console patterns.

use anyhow::{bail, Result};
use std::time::{Duration, Instant};

use super::console::Console;
use super::protocol::SHELL_READY_MARKER;

/// What boot output means; the patterns come from the caller's scenario.
pub struct BootPatterns<'a> {
    /// A line containing one means the boot succeeded. The serial test
    /// protocol's readiness marker always does.
    pub success: &'a [String],
    /// A line containing one means the boot failed.
    pub fatal: &'a [String],
    /// A line containing one reports a failed service; it is returned, not fatal.
    pub service_failures: &'a [String],
}

impl Console {
    /// Wait for boot until a success pattern appears, failing on a fatal
    /// pattern or when the console stays silent for `stall_timeout` (boot may
    /// take as long as it keeps printing). Returns the service-failure lines.
    pub fn wait_for_boot(
        &mut self,
        stall_timeout: Duration,
        patterns: &BootPatterns<'_>,
    ) -> Result<Vec<String>> {
        let mut last_output = Instant::now();
        let mut last_nudge = Instant::now();
        let mut failed_services = Vec::new();
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
            kernel_started |= line.contains("Linux version");
            let found = |list: &[String]| list.iter().find(|p| line.contains(p.as_str())).cloned();

            if found(patterns.service_failures).is_some() {
                eprintln!("  WARN: Service failure observed: {}", line.trim());
                failed_services.push(line.clone());
            }
            if let Some(pattern) = found(patterns.fatal) {
                bail!("Boot failed: {pattern}\n\nContext:\n{}", self.tail());
            }
            if line.contains(SHELL_READY_MARKER) || found(patterns.success).is_some() {
                return Ok(failed_services);
            }
        }
    }
}
