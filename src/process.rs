//! Process utilities for QEMU test management.
//!
//! Provides utilities for managing QEMU processes during testing:
//! - Killing stale QEMU processes from previous runs
//! - Acquiring exclusive locks to prevent concurrent tests

use anyhow::Result;
use std::fs::File;
use std::process::Command;
use std::time::Duration;

/// Kill any stale QEMU processes from previous test runs.
///
/// This prevents memory leaks from zombie QEMU instances that weren't properly cleaned up.
/// Searches for QEMU processes with known test-related file patterns in their command line.
pub fn kill_stale_qemu_processes() {
    let patterns = [
        "leviso-install-test.qcow2",
        "boot-hypothesis-test.qcow2",
        "levitateos.iso",
    ];

    for pattern in patterns {
        let _ = Command::new("pkill")
            .args(["-9", "-f", &format!("qemu-system-x86_64.*{}", pattern)])
            .status();
    }

    std::thread::sleep(Duration::from_millis(100));
}

/// Acquire an exclusive lock for QEMU tests.
///
/// Returns a file handle that must be kept alive for the duration of the test.
/// This prevents multiple QEMU tests from running simultaneously, which would
/// compete for system resources and cause flaky failures.
///
/// # Errors
///
/// Returns an error if another test is already holding the lock.
pub fn acquire_test_lock() -> Result<File> {
    use std::fs::OpenOptions;
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;

    let lock_path = std::path::Path::new("/tmp/leviso-install-test.lock");

    #[cfg(unix)]
    let file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o644)
        .open(lock_path)?;

    #[cfg(not(unix))]
    let file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(lock_path)?;

    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let fd = file.as_raw_fd();

        // SAFETY: flock is a standard POSIX function, fd is valid
        let result = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };

        if result != 0 {
            anyhow::bail!(
                "Another install-test is already running. \
                 Kill it with: pkill -9 -f 'qemu-system-x86_64.*leviso'"
            );
        }
    }

    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kill_stale_qemu_does_not_panic() {
        // Just verify it doesn't crash - we don't actually want to kill processes in tests
        kill_stale_qemu_processes();
    }
}
