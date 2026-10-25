use crate::harness::resource_bridge::ResourceResponse;
use crate::harness::{DependencyPreparationStatus, DependencyPreparationTelemetry};

pub(super) fn prepared_response(
    package_count: usize,
    cache_hits: usize,
    bytes: usize,
    existing: bool,
) -> ResourceResponse {
    let summary = if existing {
        format!(
            "Go verified all {package_count} go.sum module entries in the app-owned cache; no downloads were needed."
        )
    } else {
        format!(
            "Go prepared {package_count} h1-locked module entries through proxy.golang.org; the task remains offline."
        )
    };
    ResourceResponse::prepared(summary).with_preparation(DependencyPreparationTelemetry {
        status: Some(if existing {
            DependencyPreparationStatus::AlreadyAvailable
        } else {
            DependencyPreparationStatus::Prepared
        }),
        package_count: package_count as u64,
        cache_hits: cache_hits as u64,
        packages_downloaded: package_count.saturating_sub(cache_hits) as u64,
        bytes_downloaded: bytes as u64,
        ..Default::default()
    })
}
