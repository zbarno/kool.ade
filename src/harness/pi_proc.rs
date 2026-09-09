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
use std::process::{Child, Command, Stdio};
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
    let (tx, rx) = std::sync::mpsc::channel();
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..])
        .current_dir(cwd)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
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
    let stdout_reader = pipe_lines(stdout, tx.clone(), true);
    let stderr_reader = pipe_lines(stderr, tx.clone(), false);

    let watcher = handle.clone();
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
                let _ = exit_sender.send(StreamEvt::Exited(status.success()));
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    });
    // All senders are thread-owned clones; when every thread exits the
    // receiver disconnects and `next_line` returns `None`.

    Ok(ChildTask { rx, child: handle })
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
mod tests {
    use super::*;

    fn sh(script: &str) -> Vec<String> {
        vec![
            String::from("/bin/sh"),
            String::from("-c"),
            script.to_string(),
        ]
    }

    #[test]
    fn large_prompt_is_delivered_exactly_through_stdin() {
        let input = "filters: α\n$literal `text` ".repeat(80_000);
        let task = spawn_with_input(&sh("wc -c"), Path::new("/"), Some(input.clone())).unwrap();
        let mut count = None;
        loop {
            match task.poll_next(Duration::from_secs(5)).unwrap() {
                StreamEvt::Stdout(text) => count = Some(text.trim().parse::<usize>().unwrap()),
                StreamEvt::Exited(ok) => {
                    assert!(ok);
                    break;
                }
                _ => {}
            }
        }
        assert_eq!(count, Some(input.len()));
    }

    #[test]
    fn exit_follows_all_buffered_output() {
        let task = spawn(
            &sh("i=0; while [ $i -lt 12000 ]; do echo final-payload; i=$((i+1)); done"),
            Path::new("/"),
        )
        .unwrap();
        let mut lines = 0;
        loop {
            match task.poll_next(Duration::from_secs(5)).unwrap() {
                StreamEvt::Stdout(_) => lines += 1,
                StreamEvt::Exited(ok) => {
                    assert!(ok);
                    break;
                }
                _ => {}
            }
        }
        assert_eq!(lines, 12000);
    }

    #[test]
    fn echoes_lines_and_reports_success() {
        let task = spawn(&sh("printf 'a\\nb\\nc\\n'"), Path::new("/")).unwrap();
        let mut lines = Vec::new();
        let mut ok = None;
        while let Some(evt) = task.next_line(Duration::from_secs(3)) {
            match evt {
                StreamEvt::Stdout(s) => lines.push(s),
                StreamEvt::Exited(o) => ok = Some(o),
                _ => {}
            }
        }
        assert_eq!(lines, vec!["a", "b", "c"]);
        assert_eq!(ok, Some(true));
    }

    #[test]
    fn failing_child_reports_failure_with_stderr() {
        let task = spawn(&sh("echo oops >&2; exit 3"), Path::new("/")).unwrap();
        let mut saw_fail = false;
        let mut stderr = Vec::new();
        while let Some(evt) = task.next_line(Duration::from_secs(3)) {
            match evt {
                StreamEvt::Exited(o) => saw_fail = !o,
                StreamEvt::Stderr(s) => stderr.push(s),
                _ => {}
            }
        }
        assert!(saw_fail);
        assert!(stderr.iter().any(|l| l.contains("oops")));
    }

    #[test]
    fn slow_first_event_survives_short_polls() {
        // Regression: a child whose first output arrives AFTER several poll
        // windows must classify those gaps as `Pending`, never as a dead
        // stream (flat `None` used to kill healthy turns at ~200 ms).
        let task = spawn(&sh("sleep 0.4; printf 'late\\n'"), Path::new("/")).unwrap();
        let t0 = Instant::now();
        let mut pending_seen = 0usize;
        let mut got_late = false;
        loop {
            match task.poll_next(Duration::from_millis(40)) {
                Err(PollState::Pending) => pending_seen += 1,
                Ok(StreamEvt::Stdout(s)) if s == "late" => got_late = true,
                Ok(StreamEvt::Exited(_)) => break,
                Ok(_) => {}
                Err(PollState::Closed) => panic!("stream closed before the child exited"),
            }
            assert!(t0.elapsed() < Duration::from_secs(5), "drained forever");
        }
        assert!(got_late);
        assert!(
            pending_seen >= 1,
            "expected at least one Pending gap before the first line"
        );
    }

    #[test]
    fn sleeper_is_interruptible() {
        let task = spawn(&sh("sleep 30"), Path::new("/")).unwrap();
        std::thread::sleep(Duration::from_millis(150));
        task.kill();
        let settled = task.settle(Duration::from_secs(3));
        assert!(settled == Some(false) || settled.is_none());
    }
}
