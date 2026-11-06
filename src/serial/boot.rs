//! Boot waiting and detection for QEMU console.
//!
//! Provides wait_for_boot_with_patterns() with stall detection
//! and fail-fast error pattern matching.

use anyhow::{bail, Result};
use std::time::{Duration, Instant};

use super::console::Console;
use crate::patterns::SERVICE_FAILURE_PATTERNS;

impl Console {
    /// Wait for boot until a success pattern appears, failing on an error
    /// pattern or when the console stays silent for `stall_timeout` (boot
    /// may take as long as it keeps printing).
    ///
    /// With `track_service_failures`, service failures are recorded in
    /// [`Console::failed_services`] for later diagnostics instead of failing.
    pub fn wait_for_boot_with_patterns(
        &mut self,
        stall_timeout: Duration,
        success_patterns: &[&str],
        error_patterns: &[&str],
        track_service_failures: bool,
    ) -> Result<()> {
        let mut last_output = Instant::now();
        let mut last_nudge = Instant::now();
        let mut saw_uefi = false;
        let mut saw_bootloader = false;
        let mut saw_kernel = false;
        self.failed_services.clear();

        loop {
            // Nudge the serial console after the kernel started so getty
            // prompts that wait for input appear (headless OpenRC).
            if saw_kernel && last_nudge.elapsed() >= Duration::from_secs(3) {
                self.send("\n")?;
                last_nudge = Instant::now();
            }
            if last_output.elapsed() > stall_timeout {
                let phase = if saw_kernel {
                    "Kernel started but init STALLED (no output)"
                } else if saw_bootloader {
                    "Bootloader ran but kernel STALLED (no output)"
                } else if saw_uefi {
                    "UEFI ran but then STALLED (no output)"
                } else {
                    "No output received - QEMU or serial broken"
                };
                bail!(
                    "BOOT STALLED: {phase}\nNo output for {} seconds - system appears hung.\n\nLast output:\n{}",
                    stall_timeout.as_secs(),
                    self.tail()
                );
            }

            let Some(line) = self.recv_line(Duration::from_millis(500))? else {
                continue;
            };
            last_output = Instant::now();
            saw_uefi |= line.contains("UEFI") || line.contains("BdsDxe") || line.contains("EFI");
            saw_bootloader |= line.contains("systemd-boot")
                || line.contains("Loading Linux")
                || line.contains("loader");
            saw_kernel |= line.contains("Linux version")
                || line.contains("Booting Linux")
                || line.contains("KASLR");

            if track_service_failures && SERVICE_FAILURE_PATTERNS.iter().any(|p| line.contains(p)) {
                eprintln!("  WARN: Service failure observed: {}", line.trim());
                self.failed_services.push(line.clone());
            }
            if let Some(pattern) = error_patterns.iter().find(|p| line.contains(*p)) {
                bail!("Boot failed: {pattern}\n\nContext:\n{}", self.tail());
            }
            if success_patterns.iter().any(|p| line.contains(p)) {
                return Ok(());
            }
        }
    }
}
