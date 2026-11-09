use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use super::{ComputerUseSchedulerV1, ResourceRequest};

fn request(vcpus: u16) -> ResourceRequest {
    ResourceRequest {
        vcpus,
        memory_bytes: u64::from(vcpus) * 10,
        disk_bytes: u64::from(vcpus) * 20,
    }
}

fn scheduler(slots: u16, vcpus: u16) -> Arc<ComputerUseSchedulerV1> {
    ComputerUseSchedulerV1::new(slots, vcpus, u64::from(vcpus) * 10, u64::from(vcpus) * 20).unwrap()
}

#[test]
fn impossible_budgets_fail_without_entering_the_queue() {
    let scheduler = scheduler(1, 2);
    assert!(matches!(
        scheduler.acquire("session-a", request(3), &AtomicBool::new(false)),
        Err("session_resource_budget_exceeds_host_capacity")
    ));
    assert_eq!(scheduler.counts(), (0, 0));
}

#[test]
fn fifo_admission_never_overcommits_a_slot() {
    let scheduler = scheduler(1, 2);
    let first = scheduler
        .acquire("session-a", request(1), &AtomicBool::new(false))
        .unwrap();
    let (sent, received) = mpsc::channel();
    let queued_scheduler = scheduler.clone();
    let second = thread::spawn(move || {
        let permit = queued_scheduler
            .acquire("session-b", request(1), &AtomicBool::new(false))
            .unwrap();
        sent.send(()).unwrap();
        permit
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    while scheduler.counts().1 == 0 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(scheduler.counts(), (1, 1));
    assert!(received.try_recv().is_err());
    drop(first);
    received.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(scheduler.counts(), (1, 0));
    drop(second.join().unwrap());
    assert_eq!(scheduler.counts(), (0, 0));
}

#[test]
fn cancellation_removes_a_queued_session_without_releasing_another() {
    let scheduler = scheduler(1, 2);
    let active = scheduler
        .acquire("session-a", request(1), &AtomicBool::new(false))
        .unwrap();
    let cancelled = Arc::new(AtomicBool::new(false));
    let queued_cancelled = cancelled.clone();
    let queued_scheduler = scheduler.clone();
    let queued =
        thread::spawn(move || queued_scheduler.acquire("session-b", request(1), &queued_cancelled));
    let deadline = Instant::now() + Duration::from_secs(2);
    while scheduler.counts().1 == 0 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
    cancelled.store(true, Ordering::Release);
    assert!(matches!(queued.join().unwrap(), Err("cancelled")));
    assert_eq!(scheduler.counts(), (1, 0));
    drop(active);
    assert_eq!(scheduler.counts(), (0, 0));
}
