use std::sync::atomic::AtomicUsize;

pub(super) fn resolve_with_download_budget<T>(
    downloaded_bytes: &AtomicUsize,
    requested: usize,
    limit: usize,
    operation: impl FnOnce(usize) -> (anyhow::Result<T>, usize),
) -> anyhow::Result<T> {
    let reservation = crate::harness::resource_bridge::budget::reserve_downloads(
        downloaded_bytes,
        requested,
        limit,
    )?;
    let (result, actual) = operation(reservation);
    crate::harness::resource_bridge::budget::settle_downloads(
        downloaded_bytes,
        reservation,
        actual,
    );
    result
}

#[cfg(test)]
mod tests;
