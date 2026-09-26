use super::*;
use std::{
    path::Path,
    time::{Duration, Instant},
};

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
fn clean_spawn_passes_only_explicit_environment_values() {
    let task = spawn_with_input_clear_env(
        &["/usr/bin/env".into()],
        Path::new("/"),
        None,
        &[("PACKET_TEST_ONLY".into(), "present".into())],
    )
    .unwrap();
    let mut lines = Vec::new();
    loop {
        match task.poll_next(Duration::from_secs(3)).unwrap() {
            StreamEvt::Stdout(line) => lines.push(line),
            StreamEvt::Exited(ok) => {
                assert!(ok);
                break;
            }
            StreamEvt::Stderr(line) => panic!("env failed: {line}"),
        }
    }
    assert_eq!(lines, ["PACKET_TEST_ONLY=present"]);
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
