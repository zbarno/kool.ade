use super::*;

fn report(exit_code: i32) -> CapturedExecReport {
    CapturedExecReport {
        exit_code,
        stdout_tail: format!("out-{exit_code}\n"),
        stderr_tail: String::new(),
        stdout_bytes: 1,
        stderr_bytes: 0,
        stdout_truncated: false,
        stderr_truncated: false,
    }
}

#[test]
fn lane_round_trip_delivers_the_job_result() {
    let _lane_guard = test_support::lane_consumer_exclusive();
    let outcome = submit(Box::new(|| Ok(report(0)))).expect("lane delivers");
    assert_eq!(outcome.exit_code, 0);
    assert_eq!(outcome.stdout_tail, "out-0\n");
}

#[test]
fn lane_reports_in_flight_faults_as_typed_results() {
    let _lane_guard = test_support::lane_consumer_exclusive();
    let faulty = submit(Box::new(|| {
        Err(ContainedRunFault::Connect("endpoint unreachable".into()))
    }));
    let Err(fault) = faulty else {
        panic!("faulty job reported success");
    };
    assert!(matches!(fault, ContainedRunFault::Connect(_)));
    assert!(fault.describe().starts_with("contained-exec connect:"));
}

#[test]
fn lane_jobs_run_to_completion_serially_in_submission_order() {
    use std::time::Duration;
    let _lane_guard = test_support::lane_consumer_exclusive();
    let (order_tx, order_rx) = mpsc::channel::<usize>();
    let jobs: Vec<_> = (0..3)
        .rev()
        .map(|index| {
            let tx = order_tx.clone();
            Box::new(move || {
                std::thread::sleep(Duration::from_millis(index as u64 * 25));
                let _ = tx.send(index);
                Ok(report(index as i32))
            }) as LaneJob
        })
        .collect();
    let mut codes = Vec::new();
    for job in jobs {
        codes.push(submit(job).expect("lane delivers").exit_code);
    }
    // Submission order is FIFO regardless of per-job sleep length.
    assert_eq!(codes, vec![2, 1, 0]);
    let mut order = Vec::new();
    while let Ok(item) = order_rx.try_recv() {
        order.push(item);
    }
    assert_eq!(order, vec![2, 1, 0]);
}

/// A job that panics KILLS THE WORKER MID-RUN: the in-flight caller must
/// receive a typed transport fault (its relay channel drops during the
/// worker's unwind), and the NEXT submission must transparently relaunch
/// the lane — the advertised self-healing guarantee, proven end to end.
#[test]
fn panicking_job_yields_typed_connect_fault_and_next_submit_self_heals() {
    // QUARANTINED: this test temporarily kills the PROCESS-GLOBAL lane
    // worker, so it holds the consumer lock for its whole body — no
    // bystander submission can observe the death window.
    let _lane_guard = test_support::lane_consumer_exclusive();
    // PHASE 1: the panic lands on the LANE thread (plain std thread),
    // never on the test thread; the observer sees only transport loss —
    // the relay's sending half drops inside the worker's unwind.
    let fault = submit(Box::new(|| panic!("simulated worker death")))
        .expect_err("mid-run worker death must surface as a transport fault");
    assert!(
        fault.describe().contains("vanished mid-run"),
        "worker vanishment surfaces as Connect, got: {}",
        fault.describe()
    );
    // TEARDOWN GATE: join the dead worker BEFORE phase 2. Joining proves
    // the worker's frames (mailbox receiver included) are fully popped,
    // closing the sub-frame-pop window in which a raced send could
    // theoretically still hit the dying mailbox.
    test_support::retire_current_worker();
    // PHASE 2: dead-sender detection is LAZY — the slot still holds the
    // stale sender, so the immediate retry trips the broken mailbox:
    // typed fault PLUS the slot reset that makes healing possible.
    let stale =
        submit(Box::new(|| Ok(report(1)))).expect_err("stale sender is detected on the next send");
    assert!(
        stale.describe().contains("could not enqueue"),
        "retry trips the broken-mailbox reset, got: {}",
        stale.describe()
    );
    // PHASE 3: the reset slot relaunches a fresh worker on demand and
    // the lane serves normally again.
    let recovered = submit(Box::new(|| Ok(report(17))))
        .expect("lane relaunches after worker death and serves the run");
    assert_eq!(recovered.exit_code, 17);
}
