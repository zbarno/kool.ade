//! Supervision of the external harness child process: spawn, streamed line
//! delivery, cooperative cancellation, hard kill, exit bookkeeping.
//!
//! Threading model (one process, four short-lived threads):
//! * stdout reader  → sends `Stdout(line)`
//! * stderr reader  → sends `Stderr(line)`
//! * exit watcher   → steals the `Child` from the shared handle, `wait`s,
//!   then sends `Exited(success)`
//! * consumer       → `next_line()` polling from the UI worker thread
//!
//! `kill()` and the exit watcher coordinate through the same
//! `Arc<Mutex<Option<Child>>>`, so SIGKILL-before-wait is race-free.

use std::io::BufRead;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// One piped line or lifecycle event from the child.
#[derive(Debug, Clone)]
pub enum StreamEvt {
    Stdout(String),
    Stderr(String),
    /// Process terminated; payload is `true` when it exited successfully.
    Exited(bool),
}

/// Handle supervising one spawned child. Dropping it kills the child (guards
/// against leaked long-running harness processes).
pub struct ChildTask {
    rx: Receiver<StreamEvt>,
    child: Arc<Mutex<Option<Child>>>,
    exit_status: Arc<Mutex<Option<ExitStatus>>>,
    #[cfg(unix)]
    finished: Arc<std::sync::atomic::AtomicBool>,
}

/// Tri-state poll result: separates "nothing YET" (timeout) from "stream
/// is FINISHED" (disconnected), which the flat `next_line` conflates —
/// mistaking one for the other kills healthy turns whose first event takes
/// longer than one poll window to arrive (e.g. cold interpreter startup).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollState {
    /// Timed out; more events may still come.
    Pending,
    /// Every sender is gone; no further events will arrive.
    Closed,
}

impl ChildTask {
    /// One bounded wait: `Ok(event)`, `Err(Pending)` (timed out), or
    /// `Err(Closed)` (stream finished, no more events will arrive).
    pub fn poll_next(&self, timeout: Duration) -> Result<StreamEvt, PollState> {
        use std::sync::mpsc::RecvTimeoutError::{Disconnected, Timeout};
        match self.rx.recv_timeout(timeout) {
            Ok(ev) => Ok(ev),
            Err(Disconnected) => Err(PollState::Closed),
            Err(Timeout) => Err(PollState::Pending),
        }
    }

    /// Return the numeric exit code after the child has exited normally.
    pub fn exit_code(&self) -> Option<i32> {
        self.exit_status.lock().ok()?.as_ref()?.code()
    }

    /// Block until a line arrives, the process ends, or the timeout lapses.
    /// CAUTION: returns `None` for BOTH timeout and disconnection — use
    /// [`Self::poll_next`] when the distinction matters.
    pub fn next_line(&self, timeout: Duration) -> Option<StreamEvt> {
        self.rx.recv_timeout(timeout).ok()
    }

    /// Kill the child (Unix: SIGKILL). Safe to call repeatedly/pre-emptively.
    pub fn kill(&self) {
        let mut g = self.child.lock().unwrap();
        if let Some(c) = g.as_mut() {
            #[cfg(unix)]
            if !self.finished.load(std::sync::atomic::Ordering::SeqCst) {
                // Each harness/verification command owns a process group. Cancel
                // its children too, so a resumed worktree has no abandoned writer.
                unsafe extern "C" {
                    fn kill(pid: i32, signal: i32) -> i32;
                }
                unsafe {
                    kill(-(c.id() as i32), 9);
                }
            }
            let _ = c.kill();
        }
    }

    /// Drain events until termination (or `grace`, forcing a kill).
    pub fn settle(&self, grace: Duration) -> Option<bool> {
        let deadline = Instant::now() + grace;
        while Instant::now() < deadline {
            match self.poll_next(Duration::from_millis(50)) {
                Ok(StreamEvt::Exited(ok)) => return Some(ok),
                Err(PollState::Closed) => return None,
                _ => {}
            }
        }
        self.kill();
        None
    }
}

impl Drop for ChildTask {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Spawn `argv[0] argv[1..]` in `cwd` with detached pipes.
pub fn spawn(argv: &[String], cwd: &Path) -> anyhow::Result<ChildTask> {
    spawn_with_input(argv, cwd, None)
}

/// Pipe a prompt without putting its contents in the operating system argument list.
/// The writer closes stdin at EOF and never blocks the cancellation consumer.
pub fn spawn_with_input(
    argv: &[String],
    cwd: &Path,
    input: Option<String>,
) -> anyhow::Result<ChildTask> {
    spawn_with_input_env(argv, cwd, input, &[])
}

/// Pipe a prompt while adding narrowly scoped child environment values.
pub fn spawn_with_input_env(
    argv: &[String],
    cwd: &Path,
    input: Option<String>,
    env: &[(String, String)],
) -> anyhow::Result<ChildTask> {
    spawn_with_input_env_policy(argv, cwd, input, env, false, &[])
}

/// Pipe a prompt while adding child environment values and removing selected
/// inherited variables that can execute host commands.
pub fn spawn_with_input_env_excluding(
    argv: &[String],
    cwd: &Path,
    input: Option<String>,
    env: &[(String, String)],
    excluded_env: &[&str],
) -> anyhow::Result<ChildTask> {
    spawn_with_input_env_policy(argv, cwd, input, env, false, excluded_env)
}

/// Spawn with a clean environment, then add only the supplied values.
pub fn spawn_with_input_clear_env(
    argv: &[String],
    cwd: &Path,
    input: Option<String>,
    env: &[(String, String)],
) -> anyhow::Result<ChildTask> {
    spawn_with_input_env_policy(argv, cwd, input, env, true, &[])
}

fn spawn_with_input_env_policy(
    argv: &[String],
    cwd: &Path,
    input: Option<String>,
    env: &[(String, String)],
    clear_env: bool,
    excluded_env: &[&str],
) -> anyhow::Result<ChildTask> {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut cmd = Command::new(&argv[0]);
    if clear_env {
        cmd.env_clear();
    }
    cmd.args(&argv[1..])
        .current_dir(cwd)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (name, value) in env {
        cmd.env(name, value);
    }
    for name in excluded_env {
        cmd.env_remove(name);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| anyhow::anyhow!("failed to launch {}: {e}", argv[0]))?;
    if let Some(input) = input {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("no stdin pipe"))?;
        std::thread::spawn(move || {
            use std::io::Write;
            // Early pipe closure is reported through the child's exit/error stream.
            let _ = stdin.write_all(input.as_bytes());
        });
    }
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("no stdout pipe"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow::anyhow!("no stderr pipe"))?;

    let handle: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(Some(child)));
    let exit_status = Arc::new(Mutex::new(None));
    let stdout_reader = pipe_lines(stdout, tx.clone(), true);
    let stderr_reader = pipe_lines(stderr, tx.clone(), false);

    #[cfg(unix)]
    let finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
    #[cfg(unix)]
    let watcher_finished = finished.clone();
    let watcher = handle.clone();
    let watcher_exit_status = exit_status.clone();
    let exit_sender = tx.clone();
    std::thread::spawn(move || {
        // Poll `try_wait` WITHOUT taking the Child, so `KillHandle::kill`
        // stays able to signal the process afterwards.
        loop {
            let status = {
                let mut g = watcher.lock().unwrap();
                g.as_mut().and_then(|c| c.try_wait().ok().flatten())
            };
            if let Some(status) = status {
                // Final JSON can still be buffered after process exit.
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                #[cfg(unix)]
                watcher_finished.store(true, std::sync::atomic::Ordering::SeqCst);
                let success = status.success();
                if let Ok(mut exit_status) = watcher_exit_status.lock() {
                    *exit_status = Some(status);
                }
                let _ = exit_sender.send(StreamEvt::Exited(success));
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    });
    // All senders are thread-owned clones; when every thread exits the
    // receiver disconnects and `next_line` returns `None`.

    Ok(ChildTask {
        rx,
        child: handle,
        exit_status,
        #[cfg(unix)]
        finished,
    })
}

/// One reader thread per pipe: converts lines into channel events.
fn pipe_lines<R: std::io::Read + Send + 'static>(
    reader: R,
    tx: std::sync::mpsc::Sender<StreamEvt>,
    is_stdout: bool,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut br = std::io::BufReader::new(reader);
        let mut line = String::new();
        loop {
            line.clear();
            match br.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let text = line.trim_end().to_string();
                    if tx
                        .send(if is_stdout {
                            StreamEvt::Stdout(text)
                        } else {
                            StreamEvt::Stderr(text)
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests;
