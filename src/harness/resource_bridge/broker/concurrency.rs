use std::{
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread::JoinHandle,
};

use super::super::MAX_IN_FLIGHT_REQUESTS;

pub(super) fn acquire_request_slot(in_flight: &AtomicUsize) -> bool {
    in_flight
        .fetch_update(Ordering::AcqRel, Ordering::Relaxed, |active| {
            (active < MAX_IN_FLIGHT_REQUESTS).then_some(active + 1)
        })
        .is_ok()
}

pub(super) fn reap_finished_requests(requests: &Mutex<Vec<JoinHandle<()>>>) {
    let Ok(mut requests) = requests.lock() else {
        return;
    };
    let mut index = 0;
    while index < requests.len() {
        if requests[index].is_finished() {
            let finished = requests.swap_remove(index);
            let _ = finished.join();
        } else {
            index += 1;
        }
    }
}
