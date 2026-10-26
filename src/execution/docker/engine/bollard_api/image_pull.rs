use std::time::Duration;

use bollard::{models::CreateImageInfo, query_parameters::CreateImageOptionsBuilder};
use futures_util::{Stream, StreamExt};

use super::{BollardApi, failure};
use crate::execution::docker::{
    discovery::{PrerequisiteCode, PrerequisiteFailure},
    engine::{EngineSnapshot, ImagePullControl},
    images::{ImageLock, ImageObservation, SelectedImage},
};

mod retry;

const CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(100);

pub(super) async fn pull_approved_image(
    api: &BollardApi,
    image: &SelectedImage,
    expected_engine_fingerprint: &str,
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
            verify_pull_storage_capacity(image, &snapshot)
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
    image: &SelectedImage,
    snapshot: &EngineSnapshot,
) -> Result<(), PrerequisiteFailure> {
    let estimate = ImageLock::bundled()?.size_estimate(image.profile, &snapshot.architecture)?;
    let available = if snapshot.docker_storage_accessible {
        snapshot.disk_available_bytes
    } else {
        None
    }
    .ok_or_else(|| {
        failure(
            PrerequisiteCode::InsufficientDisk,
            "Docker image storage capacity could not be verified before the approved pull",
            false,
        )
    })?;
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
    Ok(())
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
            let retryable = retry::is_retryable_pull_error(&error);
            PullAttemptError {
                failure: pull_failure(retryable),
                retryable,
            }
        })?;
        if let Some(detail) = item.error_detail.as_ref() {
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
        }
    }
    if control.is_cancelled() {
        return Err(PullAttemptError::permanent(cancelled()));
    }
    Ok(())
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

fn cancelled() -> PrerequisiteFailure {
    failure(
        PrerequisiteCode::ImageUnavailable,
        "Approved image preparation was cancelled",
        true,
    )
}

#[cfg(test)]
mod tests;
