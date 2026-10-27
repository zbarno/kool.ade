use std::{io::ErrorKind, sync::mpsc, time::Duration};

use futures_util::stream;

use crate::execution::docker::engine::ImagePullControl;

use super::super::retry as pull_retry;

#[tokio::test]
async fn authorization_digest_platform_and_validation_errors_are_not_retried() {
    for message in [
        "unauthorized registry request",
        "digest verification failed",
        "no matching manifest for platform",
        "invalid image reference validation",
        "registry returned HTTP 501 Not Implemented",
        "registry returned HTTP 501 Internal Server Error",
        "registry returned HTTP 505 HTTP Version Not Supported",
    ] {
        let (progress_tx, _progress_rx) = mpsc::sync_channel(4);
        let control = ImagePullControl::new(progress_tx);
        let mut attempts = 0;
        let mut create_stream = || {
            attempts += 1;
            stream::iter(vec![Err(bollard::errors::Error::DockerStreamError {
                error: message.to_owned(),
            })])
        };

        assert!(
            pull_retry::pull_with_retries(
                || std::future::ready(Ok(())),
                &mut create_stream,
                &control,
            )
            .await
            .is_err()
        );
        assert_eq!(attempts, 1, "unexpected retry for {message}");
    }
}

#[tokio::test]
async fn non_transient_server_responses_are_not_retried() {
    for status_code in [501, 505] {
        let (progress_tx, _progress_rx) = mpsc::sync_channel(4);
        let control = ImagePullControl::new(progress_tx);
        let mut attempts = 0;
        let mut create_stream = || {
            attempts += 1;
            stream::iter(vec![Err(
                bollard::errors::Error::DockerResponseServerError {
                    status_code,
                    message: "registry operation is not implemented".into(),
                },
            )])
        };

        assert!(
            pull_retry::pull_with_retries(
                || std::future::ready(Ok(())),
                &mut create_stream,
                &control,
            )
            .await
            .is_err()
        );
        assert_eq!(attempts, 1);
    }
}

#[test]
fn standard_network_unreachable_errors_are_retryable() {
    for kind in [
        ErrorKind::NetworkUnreachable,
        ErrorKind::HostUnreachable,
        ErrorKind::NetworkDown,
    ] {
        let error = bollard::errors::Error::IOError {
            err: std::io::Error::from(kind),
        };
        assert!(pull_retry::is_retryable_pull_error(&error), "{kind:?}");
    }
}

#[test]
fn retry_backoff_is_exponential_and_capped() {
    assert_eq!(pull_retry::retry_delay(1), Duration::from_millis(500));
    assert_eq!(pull_retry::retry_delay(2), Duration::from_secs(1));
    assert_eq!(pull_retry::retry_delay(3), Duration::from_secs(1));
}
