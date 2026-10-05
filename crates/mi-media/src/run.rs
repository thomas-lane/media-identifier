//! Running ffmpeg and ffprobe as child processes with cancellation.
//!
//! Every invocation goes through [`spawn`] so that all child processes get the same treatment:
//! no stdin, no console window on Windows, stderr captured (its tail becomes the error message),
//! and the child killed when the job's [`CancelFlag`] is set.

use std::collections::VecDeque;
use std::ffi::OsStr;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, ChildStdout, Command, ExitStatus, Stdio};
use std::thread::JoinHandle;
use std::time::Duration;

use mi_types::CancelFlag;

use crate::{MediaError, Sidecars, Tool};

/// How many stderr lines are kept for error messages.
const STDERR_TAIL_LINES: usize = 12;

/// How often a waiting caller checks the cancel flag.
const POLL_INTERVAL: Duration = Duration::from_millis(25);

/// A running tool with stdout piped and stderr collected in the background.
pub(crate) struct Running {
    tool: Tool,
    input: std::path::PathBuf,
    child: Child,
    stderr: Option<JoinHandle<Vec<String>>>,
}

/// Starts `tool` with `args`. `input` is only used in error messages.
pub(crate) fn spawn<I, S>(
    sidecars: &Sidecars,
    tool: Tool,
    input: &Path,
    args: I,
) -> crate::Result<Running>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = Command::new(sidecars.path(tool));
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_console_window(&mut command);
    tracing::debug!(?command, "running {tool}");
    let mut child = command.spawn().map_err(|e| MediaError::ToolFailed {
        tool,
        path: input.to_path_buf(),
        message: format!("could not start {}: {e}", sidecars.path(tool).display()),
    })?;
    let stderr = child.stderr.take().map(|pipe| {
        std::thread::spawn(move || {
            let mut tail = VecDeque::with_capacity(STDERR_TAIL_LINES);
            for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                if tail.len() == STDERR_TAIL_LINES {
                    tail.pop_front();
                }
                tail.push_back(line);
            }
            tail.into_iter().collect()
        })
    });
    Ok(Running {
        tool,
        input: input.to_path_buf(),
        child,
        stderr,
    })
}

impl Running {
    /// Takes the child's stdout. Call at most once.
    pub(crate) fn stdout(&mut self) -> ChildStdout {
        self.child
            .stdout
            .take()
            .expect("stdout is piped and taken once")
    }

    /// Kills the child (ignoring errors: it may already have exited) and reaps it.
    pub(crate) fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    /// Waits for the child, killing it if `cancel` is set. Returns `Cancelled` on cancellation and
    /// `ToolFailed` (with the stderr tail) on a non-zero exit.
    pub(crate) fn finish(mut self, cancel: &CancelFlag) -> crate::Result<()> {
        let status = loop {
            if cancel.is_cancelled() {
                self.kill();
                return Err(MediaError::Cancelled);
            }
            match self.child.try_wait()? {
                Some(status) => break status,
                None => std::thread::sleep(POLL_INTERVAL),
            }
        };
        let tail = self
            .stderr
            .take()
            .and_then(|h| h.join().ok())
            .unwrap_or_default();
        self.check_status(status, &tail)
    }

    fn check_status(&self, status: ExitStatus, stderr_tail: &[String]) -> crate::Result<()> {
        if status.success() {
            return Ok(());
        }
        let detail = stderr_tail.join("\n");
        Err(MediaError::ToolFailed {
            tool: self.tool,
            path: self.input.clone(),
            message: if detail.trim().is_empty() {
                format!("exited with {status}")
            } else {
                format!("exited with {status}: {}", detail.trim())
            },
        })
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        // A Running dropped before `finish` (an early return or a panic) must not leave a
        // decoder running in the background.
        if matches!(self.child.try_wait(), Ok(None)) {
            self.kill();
        }
    }
}

/// Runs a tool to completion and returns everything it wrote to stdout. Reads stdout on a helper
/// thread so the calling thread can watch `cancel`.
pub(crate) fn output<I, S>(
    sidecars: &Sidecars,
    tool: Tool,
    input: &Path,
    args: I,
    cancel: &CancelFlag,
) -> crate::Result<Vec<u8>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    crate::check_cancel(cancel)?;
    let mut running = spawn(sidecars, tool, input, args)?;
    let mut stdout = running.stdout();
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        stdout.read_to_end(&mut buf).map(|_| buf)
    });
    running.finish(cancel)?;
    // The child has exited, so its stdout is closed and the reader finishes promptly.
    let bytes = reader
        .join()
        .map_err(|_| std::io::Error::other("stdout reader panicked"))??;
    Ok(bytes)
}

#[cfg(windows)]
fn hide_console_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    // CREATE_NO_WINDOW: the app is a GUI program, so without this every ffmpeg call would flash
    // a console window.
    command.creation_flags(0x0800_0000);
}

#[cfg(not(windows))]
fn hide_console_window(_command: &mut Command) {}
