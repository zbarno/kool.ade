use std::time::Duration;

use bollard::{models::CreateImageInfo, query_parameters::CreateImageOptionsBuilder};
use futures_util::{Stream, StreamExt};

use super::{BollardApi, failure};
use crate::execution::docker::{
    discovery::{PrerequisiteCode, PrerequisiteFailure},
    engine::{EngineSnapshot, ImagePullControl},
    images::{
        DESKTOP_CAPACITY_PROBE_MAX_PULL_BYTES, DESKTOP_CAPACITY_PROBE_MAX_STORAGE_BYTES, ImageLock,
        ImageObservation, SelectedImage,
    },
};

mod retry;

const CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(100);

pub(super) async fn pull_approved_image(
    api: &BollardApi,
    image: &SelectedImage,
    expected_engine_fingerprint: &str,
    verified_capacity_bytes: Option<u64>,
    control: &ImagePullControl,
) -> Result<ImageObservation, PrerequisiteFailure> {
    if control.is_cancelled() {
        return Err(cancelled());
    }
    api.require_approved_engine(image, expected_engine_fingerprint)
        .await?;
    match api.inspect_image_without_pull(image).await {
        Ok(observation) => return Ok(observation),
        Err(error) if error.code == PrerequisiteCode::ImageUnavailable => {}
        Err(error) => return Err(error),
    }
    if control.is_cancelled() {
        return Err(cancelled());
    }
    retry::pull_with_retries(
        || async move {
            let snapshot = api
                .approved_engine_snapshot(image, expected_engine_fingerprint)
                .await?;
            let bounded_pull_limit = verify_pull_storage_capacity(
                api.endpoint.is_desktop(),
                image,
                &snapshot,
                verified_capacity_bytes,
            )?;
            if let Some(maximum) = bounded_pull_limit {
                control.restrict_reported_bytes(maximum);
            }
            Ok(())
        },
        || {
            let options = CreateImageOptionsBuilder::default()
                .from_image(&image.reference)
                .platform(&image.platform)
                .build();
            api.docker.create_image(Some(options), None, None)
        },
        control,
    )
    .await?;
    api.inspect_image_without_pull(image).await.map_err(|_| {
        failure(
            PrerequisiteCode::ImageUnavailable,
            "Pulled executor image could not be inspected",
            true,
        )
    })
}

fn verify_pull_storage_capacity(
    desktop: bool,
    image: &SelectedImage,
    snapshot: &EngineSnapshot,
    verified_capacity_bytes: Option<u64>,
) -> Result<Option<u64>, PrerequisiteFailure> {
    let estimate = ImageLock::bundled()?.size_estimate(image.profile, &snapshot.architecture)?;
    let available = if snapshot.docker_storage_accessible {
        snapshot.disk_available_bytes
    } else if desktop && snapshot.docker_desktop {
        verified_capacity_bytes
    } else {
        None
    };
    if available.is_none()
        && desktop
        && snapshot.docker_desktop
        && !snapshot.docker_storage_accessible
        && snapshot.disk_available_bytes.is_none()
        && image.profile == crate::execution::docker::images::RuntimeProfile::General
        && estimate.estimated_pull_bytes <= DESKTOP_CAPACITY_PROBE_MAX_PULL_BYTES
        && estimate.estimated_storage_bytes <= DESKTOP_CAPACITY_PROBE_MAX_STORAGE_BYTES
    {
        // The exact-lock General image is a size-bounded bootstrap probe. It
        // may fail with ENOSPC; readiness still requires a later VM sample.
        return Ok(Some(DESKTOP_CAPACITY_PROBE_MAX_PULL_BYTES));
    }
    let Some(available) = available else {
        return Err(failure(
            PrerequisiteCode::InsufficientDisk,
            "Docker image storage capacity could not be verified before the approved pull",
            false,
        ));
    };
    let required = snapshot
        .minimum_disk_bytes
        .checked_add(estimate.estimated_storage_bytes)
        .ok_or_else(|| {
            failure(
                PrerequisiteCode::InsufficientDisk,
                "Approved image storage requirement is outside the supported size bound",
                false,
            )
        })?;
    if available < required {
        return Err(failure(
            PrerequisiteCode::InsufficientDisk,
            "Docker image storage does not have enough free space for the approved runtime image",
            false,
        ));
    }
    Ok(None)
}

#[cfg(test)]
async fn consume_pull_stream<S>(
    stream: S,
    control: &ImagePullControl,
) -> Result<(), PrerequisiteFailure>
where
    S: Stream<Item = Result<CreateImageInfo, bollard::errors::Error>> + Unpin,
{
    consume_pull_attempt(stream, control)
        .await
        .map_err(|error| error.failure)
}

async fn consume_pull_attempt<S>(
    mut stream: S,
    control: &ImagePullControl,
) -> Result<(), PullAttemptError>
where
    S: Stream<Item = Result<CreateImageInfo, bollard::errors::Error>> + Unpin,
{
    if control.is_cancelled() {
        return Err(PullAttemptError::permanent(cancelled()));
    }
    let mut cancellation_poll = tokio::time::interval(CANCEL_POLL_INTERVAL);
    cancellation_poll.tick().await;
    loop {
        let pulled = tokio::select! {
            item = stream.next() => item,
            _ = cancellation_poll.tick() => {
                if control.is_cancelled() {
                    return Err(PullAttemptError::permanent(cancelled()));
                }
                continue;
            }
        };
        let Some(pulled) = pulled else {
            break;
        };
        let item = pulled.map_err(|error| {
            if is_storage_exhaustion_error(&error) {
                return PullAttemptError::permanent(storage_exhausted());
            }
            let retryable = retry::is_retryable_pull_error(&error);
            PullAttemptError {
                failure: pull_failure(retryable),
                retryable,
            }
        })?;
        if let Some(detail) = item.error_detail.as_ref() {
            if detail
                .message
                .as_deref()
                .is_some_and(is_storage_exhaustion_hint)
            {
                return Err(PullAttemptError::permanent(storage_exhausted()));
            }
            let retryable =
                retry::is_retryable_error_detail(detail.code, detail.message.as_deref());
            return Err(PullAttemptError {
                failure: pull_failure(retryable),
                retryable,
            });
        }
        if let Some(detail) = item.progress_detail.as_ref() {
            control
                .record_progress(item.id.as_deref(), detail.current, detail.total)
                .map_err(PullAttemptError::permanent)?;
        } else if item.id.is_some() {
            control
                .record_unmeasured_layer(
                    item.id.as_deref(),
                    terminal_layer_status(item.status.as_deref()),
                )
                .map_err(PullAttemptError::permanent)?;
        }
    }
    if control.is_cancelled() {
        return Err(PullAttemptError::permanent(cancelled()));
    }
    control
        .finish_pull_attempt()
        .map_err(PullAttemptError::permanent)?;
    Ok(())
}

fn terminal_layer_status(status: Option<&str>) -> bool {
    status.is_some_and(|status| {
        let status = status.to_ascii_lowercase();
        status.contains("already exists")
    })
}

struct PullAttemptError {
    failure: PrerequisiteFailure,
    retryable: bool,
}

impl PullAttemptError {
    fn permanent(failure: PrerequisiteFailure) -> Self {
        Self {
            failure,
            retryable: false,
        }
    }
}

fn pull_failure(retryable: bool) -> PrerequisiteFailure {
    failure(
        PrerequisiteCode::ImageUnavailable,
        "Trusted executor image pull failed; check registry access and retry",
        retryable,
    )
}

fn is_storage_exhaustion_error(error: &bollard::errors::Error) -> bool {
    match error {
        bollard::errors::Error::DockerStreamError { error } => is_storage_exhaustion_hint(error),
        bollard::errors::Error::DockerResponseServerError { message, .. } => {
            is_storage_exhaustion_hint(message)
        }
        bollard::errors::Error::IOError { err } => is_storage_exhaustion_hint(&err.to_string()),
        _ => false,
    }
}

fn is_storage_exhaustion_hint(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    [
        "no space left on device",
        "disk quota exceeded",
        "not enough space",
        "insufficient storage",
    ]
    .iter()
    .any(|hint| message.contains(hint))
}

fn storage_exhausted() -> PrerequisiteFailure {
    failure(
        PrerequisiteCode::InsufficientDisk,
        "Docker ran out of VM storage while preparing the approved image; free space and retry",
        false,
    )
}

fn cancelled() -> PrerequisiteFailure {
    failure(
        PrerequisiteCode::ImageUnavailable,
        "Approved image preparation was cancelled",
        true,
    )
}

#[cfg(test)]
mod tests;
