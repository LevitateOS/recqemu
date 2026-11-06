//! The serial console of a QEMU guest: lines in, text out, with a transcript.

use anyhow::{bail, Context, Result};
use std::io::{Read, Write};
use std::process::Child;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

/// How many transcript lines an error shows.
pub(crate) const CONTEXT_LINES: usize = 30;

/// Result of executing a command in QEMU.
#[derive(Debug)]
pub struct CommandResult {
    /// Whether the command completed.
    pub completed: bool,
    /// Exit code (0 = success).
    pub exit_code: i32,
    /// Output from the command.
    pub output: String,
    /// Whether execution was aborted due to fatal error pattern.
    pub aborted_on_error: bool,
}

impl CommandResult {
    /// Check if the command succeeded.
    pub fn success(&self) -> bool {
        self.completed && self.exit_code == 0 && !self.aborted_on_error
    }
}

/// Console controller for QEMU serial I/O (`-serial mon:stdio`, piped).
pub struct Console {
    stdin: std::process::ChildStdin,
    rx: Receiver<String>,
    /// Every line received, for diagnostics.
    transcript: Vec<String>,
    /// Services that failed during boot (tracked for diagnostics).
    pub(crate) failed_services: Vec<String>,
}

impl Console {
    /// Take over a QEMU process spawned with piped stdin/stdout.
    pub fn new(child: &mut Child) -> Result<Self> {
        let stdin = child.stdin.take().context("Failed to get QEMU stdin")?;
        let stdout = child.stdout.take().context("Failed to get QEMU stdout")?;
        let (chunk_tx, chunk_rx) = mpsc::channel::<Vec<u8>>();
        std::thread::spawn(move || {
            let mut stdout = stdout;
            let mut buf = [0u8; 4096];
            while let Ok(n) = stdout.read(&mut buf) {
                if n == 0 || chunk_tx.send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || assemble_lines(chunk_rx, tx));
        Ok(Self {
            stdin,
            rx,
            transcript: Vec::new(),
            failed_services: Vec::new(),
        })
    }

    /// The next line within `timeout` (`None` on timeout); every line goes
    /// into the transcript. Fails when QEMU's output closed.
    pub(crate) fn recv_line(&mut self, timeout: Duration) -> Result<Option<String>> {
        match self.rx.recv_timeout(timeout) {
            Ok(line) => {
                self.transcript.push(line.clone());
                Ok(Some(line))
            }
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => bail!(
                "QEMU serial output closed (process died?)\n\nLast output:\n{}",
                self.tail()
            ),
        }
    }

    /// Type `text` into the guest.
    pub(crate) fn send(&mut self, text: &str) -> Result<()> {
        self.stdin.write_all(text.as_bytes())?;
        self.stdin.flush()?;
        Ok(())
    }

    /// The last [`CONTEXT_LINES`] lines received.
    pub(crate) fn tail(&self) -> String {
        let start = self.transcript.len().saturating_sub(CONTEXT_LINES);
        self.transcript[start..].join("\n")
    }

    /// Get any failed services that were observed during boot.
    pub fn failed_services(&self) -> &[String] {
        &self.failed_services
    }
}

/// How long a line without its newline waits before it is delivered anyway:
/// prompts (`login: `, `Password: `) end without one.
const PARTIAL_LINE_DELAY: Duration = Duration::from_millis(100);

/// Turn raw output into lines; a trailing partial line is delivered once the
/// output has been quiet for [`PARTIAL_LINE_DELAY`].
fn assemble_lines(chunks: Receiver<Vec<u8>>, lines: Sender<String>) {
    let mut pending: Vec<u8> = Vec::new();
    loop {
        let closed = match chunks.recv_timeout(PARTIAL_LINE_DELAY) {
            Ok(chunk) => {
                pending.extend_from_slice(&chunk);
                false
            }
            Err(RecvTimeoutError::Timeout) => {
                if pending.is_empty() {
                    continue;
                }
                true
            }
            Err(RecvTimeoutError::Disconnected) => {
                if !pending.is_empty() {
                    let _ = lines.send(line_text(&pending));
                }
                return;
            }
        };
        while let Some(end) = pending.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = pending.drain(..=end).collect();
            if lines.send(line_text(&line[..end])).is_err() {
                return;
            }
        }
        if closed && !pending.is_empty() {
            if lines.send(line_text(&pending)).is_err() {
                return;
            }
            pending.clear();
        }
    }
}

fn line_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim_end_matches('\r').to_string()
}
