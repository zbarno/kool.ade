use std::{
    sync::{Arc, atomic::Ordering, mpsc},
    time::Duration,
};

use futures_util::stream;

use crate::execution::docker::engine::ImagePullControl;

use super::super::retry as pull_retry;
use super::{error_detail, progress};

#[tokio::test]
async fn transient_transport_failure_retries_without_inflating_progress() {
    let (progress_tx, progress_rx) = mpsc::sync_channel(16);
    let control = ImagePullControl::new(progress_tx);
    let mut attempts = 0;
    let mut create_stream = || {
        attempts += 1;
        if attempts == 1 {
            stream::iter(vec![
                progress(40),
                Err(bollard::errors::Error::RequestTimeoutError),
            ])
        } else {
            stream::iter(vec![progress(20), progress(80)])
        }
    };

    pull_retry::pull_with_retries(|| std::future::ready(Ok(())), &mut create_stream, &control)
        .await
        .unwrap();

    assert_eq!(attempts, 2);
    let progress = progress_rx.try_iter().collect::<Vec<_>>();
    let current = progress
        .iter()
        .map(|item| item.current_bytes.unwrap())
        .collect::<Vec<_>>();
    assert_eq!(current, vec![40, 40, 80]);
    assert!(progress.iter().all(|item| item.total_bytes == Some(100)));
    assert!(progress.len() <= 128);
}

#[tokio::test]
async fn retryable_server_response_retries_once_and_succeeds() {
    let (progress_tx, _progress_rx) = mpsc::sync_channel(4);
    let control = ImagePullControl::new(progress_tx);
    let mut attempts = 0;
    let mut create_stream = || {
        attempts += 1;
        if attempts == 1 {
            stream::iter(vec![Err(
                bollard::errors::Error::DockerResponseServerError {
                    status_code: 503,
                    message: "temporary registry outage".into(),
                },
            )])
        } else {
            stream::iter(Vec::<
                Result<bollard::models::CreateImageInfo, bollard::errors::Error>,
            >::new())
        }
    };

    pull_retry::pull_with_retries(|| std::future::ready(Ok(())), &mut create_stream, &control)
        .await
        .unwrap();

    assert_eq!(attempts, 2);
}

#[tokio::test]
async fn stream_error_details_retry_transient_503_but_not_auth_401() {
    let (progress_tx, _progress_rx) = mpsc::sync_channel(4);
    let control = ImagePullControl::new(progress_tx);
    let mut transient_attempts = 0;
    let mut transient_stream = || {
        transient_attempts += 1;
        if transient_attempts == 1 {
            stream::iter(vec![error_detail(
                Some(503),
                "temporary registry unavailable",
            )])
        } else {
            stream::iter(Vec::<
                Result<bollard::models::CreateImageInfo, bollard::errors::Error>,
            >::new())
        }
    };
    pull_retry::pull_with_retries(
        || std::future::ready(Ok(())),
        &mut transient_stream,
        &control,
    )
    .await
    .unwrap();
    assert_eq!(transient_attempts, 2);

    let (progress_tx, _progress_rx) = mpsc::sync_channel(4);
    let control = ImagePullControl::new(progress_tx);
    let mut auth_attempts = 0;
    let mut auth_stream = || {
        auth_attempts += 1;
        stream::iter(vec![error_detail(
            Some(401),
            "unauthorized registry request",
        )])
    };
    assert!(
        pull_retry::pull_with_retries(|| std::future::ready(Ok(())), &mut auth_stream, &control,)
            .await
            .is_err()
    );
    assert_eq!(auth_attempts, 1);
}

#[tokio::test]
async fn error_details_without_status_use_only_bounded_sanitized_hints() {
    let (progress_tx, _progress_rx) = mpsc::sync_channel(4);
    let control = ImagePullControl::new(progress_tx);
    let mut transient_attempts = 0;
    let mut transient_stream = || {
        transient_attempts += 1;
        if transient_attempts == 1 {
            stream::iter(vec![error_detail(None, "temporary registry unavailable")])
        } else {
            stream::iter(Vec::<
                Result<bollard::models::CreateImageInfo, bollard::errors::Error>,
            >::new())
        }
    };
    pull_retry::pull_with_retries(
        || std::future::ready(Ok(())),
        &mut transient_stream,
        &control,
    )
    .await
    .unwrap();
    assert_eq!(transient_attempts, 2);

    let (progress_tx, _progress_rx) = mpsc::sync_channel(4);
    let control = ImagePullControl::new(progress_tx);
    let secret_marker = "private-registry-detail";
    let mut permanent_attempts = 0;
    let mut permanent_stream = || {
        permanent_attempts += 1;
        stream::iter(vec![error_detail(
            None,
            &format!("digest mismatch: {secret_marker}"),
        )])
    };
    let error = pull_retry::pull_with_retries(
        || std::future::ready(Ok(())),
        &mut permanent_stream,
        &control,
    )
    .await
    .unwrap_err();
    assert_eq!(permanent_attempts, 1);
    assert!(!error.message.contains(secret_marker));

    let (progress_tx, _progress_rx) = mpsc::sync_channel(4);
    let control = ImagePullControl::new(progress_tx);
    let long_message = format!("connection reset by peer {}", "x".repeat(2048));
    let mut oversized_attempts = 0;
    let mut oversized_stream = || {
        oversized_attempts += 1;
        stream::iter(vec![error_detail(None, &long_message)])
    };
    assert!(
        pull_retry::pull_with_retries(
            || std::future::ready(Ok(())),
            &mut oversized_stream,
            &control,
        )
        .await
        .is_err()
    );
    assert_eq!(oversized_attempts, 1);
}

#[tokio::test]
async fn retry_attempts_are_bounded() {
    let (progress_tx, _progress_rx) = mpsc::sync_channel(4);
    let control = ImagePullControl::new(progress_tx);
    let mut attempts = 0;
    let mut create_stream = || {
        attempts += 1;
        stream::iter(vec![Err(bollard::errors::Error::RequestTimeoutError)])
    };

    assert!(
        pull_retry::pull_with_retries(|| std::future::ready(Ok(())), &mut create_stream, &control,)
            .await
            .is_err()
    );
    assert_eq!(attempts, pull_retry::MAX_PULL_ATTEMPTS);
}

#[tokio::test]
async fn cancellation_during_backoff_prevents_another_attempt() {
    let (progress_tx, _progress_rx) = mpsc::sync_channel(4);
    let control = ImagePullControl::new(progress_tx);
    let pull_control = control.clone();
    let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let worker_attempts = attempts.clone();
    let pull = tokio::spawn(async move {
        pull_retry::pull_with_retries(
            || std::future::ready(Ok(())),
            || {
                worker_attempts.fetch_add(1, Ordering::SeqCst);
                stream::iter(vec![Err(bollard::errors::Error::RequestTimeoutError)])
            },
            &pull_control,
        )
        .await
    });

    while attempts.load(Ordering::SeqCst) == 0 {
        tokio::task::yield_now().await;
    }
    tokio::time::sleep(Duration::from_millis(10)).await;
    control.cancel();

    let error = pull.await.unwrap().unwrap_err();
    assert!(error.message.contains("cancelled"));
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
}
