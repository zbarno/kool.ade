use super::resolve_with_download_budget;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc,
};

#[test]
fn concurrent_npm_additions_reserve_remaining_bytes_before_downloading() {
    let downloaded = Arc::new(AtomicUsize::new(0));
    let (reserved_tx, reserved_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let worker_bytes = downloaded.clone();
    let first = std::thread::spawn(move || {
        resolve_with_download_budget(&worker_bytes, 10, 10, |reservation| {
            reserved_tx.send(reservation).unwrap();
            release_rx.recv().unwrap();
            (Ok(()), 7)
        })
    });

    assert_eq!(reserved_rx.recv().unwrap(), 10);
    let second_started = AtomicBool::new(false);
    let second = resolve_with_download_budget(&downloaded, 10, 10, |reservation| {
        second_started.store(true, Ordering::SeqCst);
        (Ok(()), reservation)
    });
    assert!(second.is_err());
    assert!(!second_started.load(Ordering::SeqCst));

    release_tx.send(()).unwrap();
    first.join().unwrap().unwrap();
    assert_eq!(downloaded.load(Ordering::Acquire), 7);
    resolve_with_download_budget(&downloaded, 10, 10, |reservation| {
        assert_eq!(reservation, 3);
        (Ok(()), 3)
    })
    .unwrap();
    assert_eq!(downloaded.load(Ordering::Acquire), 10);
}
