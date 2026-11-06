//! The serial console of a guest: lines in, text out, with a transcript.

use anyhow::{bail, Context, Result};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

/// How many transcript lines an error shows.
pub(crate) const CONTEXT_LINES: usize = 30;

/// Result of executing a command in QEMU.
#[derive(Debug)]
pub struct CommandResult {
    /// False when the timeout ran out before the command finished.
    pub completed: bool,
    /// -1 when the command did not complete or its status was unreadable.
    pub exit_code: i32,
    pub output: String,
}

impl CommandResult {
    pub fn success(&self) -> bool {
        self.completed && self.exit_code == 0
    }
}

/// Console controller for a guest's serial port.
pub struct Console {
    writer: Box<dyn Write + Send>,
    rx: Receiver<String>,
    /// Every line received, for diagnostics.
    transcript: Vec<String>,
}

impl Console {
    /// Connect to a serial port exposed as a unix socket (a libvirt domain's
    /// `SerialTarget::Socket`), waiting up to `timeout` for it to appear.
    pub fn connect(socket: &Path, timeout: Duration) -> Result<Self> {
        let started = Instant::now();
        let stream = loop {
            match UnixStream::connect(socket) {
                Ok(stream) => break stream,
                Err(err) if started.elapsed() >= timeout => {
                    return Err(err).with_context(|| format!("connecting to the serial socket '{}'", socket.display()));
                }
                Err(_) => std::thread::sleep(Duration::from_millis(100)),
            }
        };
        let reader = stream.try_clone().context("duplicating the serial socket")?;
        Ok(Self::from_streams(reader, stream))
    }

    /// Drive a serial port given its output and input streams.
    pub fn from_streams(output: impl Read + Send + 'static, input: impl Write + Send + 'static) -> Self {
        let (chunk_tx, chunk_rx) = mpsc::channel::<Vec<u8>>();
        std::thread::spawn(move || {
            let mut output = output;
            let mut buf = [0u8; 4096];
            while let Ok(n) = output.read(&mut buf) {
                if n == 0 || chunk_tx.send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || assemble_lines(chunk_rx, tx));
        Self {
            writer: Box::new(input),
            rx,
            transcript: Vec::new(),
        }
    }

    /// The next line within `timeout` (`None` on timeout); every line goes
    /// into the transcript. Fails when QEMU's output closed.
    pub fn recv_line(&mut self, timeout: Duration) -> Result<Option<String>> {
        match self.rx.recv_timeout(timeout) {
            Ok(line) => {
                self.transcript.push(line.clone());
                Ok(Some(line))
            }
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => bail!(
                "the guest's serial output closed (the VM stopped?)\n\nLast output:\n{}",
                self.tail()
            ),
        }
    }

    /// Type `text` into the guest.
    pub fn send(&mut self, text: &str) -> Result<()> {
        self.writer.write_all(text.as_bytes())?;
        self.writer.flush()?;
        Ok(())
    }

    /// The last [`CONTEXT_LINES`] lines received.
    pub(crate) fn tail(&self) -> String {
        let start = self.transcript.len().saturating_sub(CONTEXT_LINES);
        self.transcript[start..].join("\n")
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
