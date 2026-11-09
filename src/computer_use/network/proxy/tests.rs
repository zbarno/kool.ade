use super::is_active;
use std::{
    sync::atomic::{AtomicBool, AtomicU64},
    time::{Duration, Instant},
};

#[test]
fn research_tunnels_fail_closed_on_pause_revoke_expiry_and_epoch_change() {
    let stop = AtomicBool::new(false);
    let paused = AtomicBool::new(false);
    let epoch = AtomicU64::new(3);
    let started = Instant::now();
    let lifetime = Duration::from_secs(2);

    assert!(is_active(&stop, &paused, &epoch, 3, started, lifetime));
    paused.store(true, std::sync::atomic::Ordering::Release);
    assert!(!is_active(&stop, &paused, &epoch, 3, started, lifetime));
    paused.store(false, std::sync::atomic::Ordering::Release);
    epoch.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    assert!(!is_active(&stop, &paused, &epoch, 3, started, lifetime));
    assert!(!is_active(
        &stop,
        &paused,
        &epoch,
        4,
        started,
        Duration::ZERO
    ));
    stop.store(true, std::sync::atomic::Ordering::Release);
    assert!(!is_active(&stop, &paused, &epoch, 4, started, lifetime));
}
