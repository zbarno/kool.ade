use std::sync::atomic::{AtomicUsize, Ordering};

pub(super) fn admit_request(count: &AtomicUsize, limit: usize) -> anyhow::Result<()> {
    count
        .fetch_update(Ordering::AcqRel, Ordering::Relaxed, |used| {
            (used < limit).then_some(used + 1)
        })
        .map(|_| ())
        .map_err(|_| anyhow::anyhow!("Resource request limit reached for this task run"))
}

pub(super) fn reserve_downloads(
    used: &AtomicUsize,
    requested: usize,
    limit: usize,
) -> anyhow::Result<usize> {
    loop {
        let current = used.load(Ordering::Acquire);
        anyhow::ensure!(
            current < limit,
            "Resource download budget reached for this task run"
        );
        let reservation = requested.min(limit - current);
        if used
            .compare_exchange_weak(
                current,
                current + reservation,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
        {
            return Ok(reservation);
        }
    }
}

pub(super) fn settle_downloads(used: &AtomicUsize, reservation: usize, actual: usize) {
    used.fetch_sub(reservation.saturating_sub(actual), Ordering::AcqRel);
}

#[cfg(test)]
mod tests {
    use super::{admit_request, reserve_downloads, settle_downloads};
    use std::sync::{Arc, atomic::AtomicUsize};

    #[test]
    fn concurrent_download_reservations_cannot_exceed_the_session_limit() {
        let used = Arc::new(AtomicUsize::new(0));
        let workers = (0..8)
            .map(|_| {
                let used = used.clone();
                std::thread::spawn(move || reserve_downloads(&used, 8, 16).ok())
            })
            .collect::<Vec<_>>();
        let reservations = workers
            .into_iter()
            .filter_map(|worker| worker.join().unwrap())
            .sum::<usize>();
        assert_eq!(reservations, 16);
        assert_eq!(used.load(std::sync::atomic::Ordering::Acquire), 16);
        settle_downloads(&used, 8, 3);
        assert_eq!(used.load(std::sync::atomic::Ordering::Acquire), 11);
    }

    #[test]
    fn request_admission_is_bounded_under_concurrency() {
        let count = Arc::new(AtomicUsize::new(0));
        let workers = (0..32)
            .map(|_| {
                let count = count.clone();
                std::thread::spawn(move || admit_request(&count, 7).is_ok())
            })
            .collect::<Vec<_>>();
        assert_eq!(
            workers
                .into_iter()
                .map(|worker| worker.join().unwrap())
                .filter(|passed| *passed)
                .count(),
            7
        );
    }
}
