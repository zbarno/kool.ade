use std::{future::Future, io::ErrorKind, time::Duration};

use bollard::models::CreateImageInfo;
use futures_util::Stream;

use crate::execution::docker::{discovery::PrerequisiteFailure, engine::ImagePullControl};

use super::{cancelled, consume_pull_attempt};

pub(super) const MAX_PULL_ATTEMPTS: usize = 3;
const BASE_RETRY_DELAY: Duration = Duration::from_millis(500);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(1);
const RETRY_CANCEL_POLL: Duration = Duration::from_millis(10);
const MAX_ERROR_HINT_BYTES: usize = 2048;

pub(super) async fn pull_with_retries<C, CF, F, S>(
    mut verify_engine: C,
    mut create_stream: F,
    control: &ImagePullControl,
) -> Result<(), PrerequisiteFailure>
where
    C: FnMut() -> CF,
    CF: Future<Output = Result<(), PrerequisiteFailure>>,
    F: FnMut() -> S,
    S: Stream<Item = Result<CreateImageInfo, bollard::errors::Error>> + Unpin,
{
    for attempt in 1..=MAX_PULL_ATTEMPTS {
        if control.is_cancelled() {
            return Err(cancelled());
        }
        verify_engine().await?;
        if control.is_cancelled() {
            return Err(cancelled());
        }
        match consume_pull_attempt(create_stream(), control).await {
            Ok(()) => return Ok(()),
            Err(error) if error.retryable && attempt < MAX_PULL_ATTEMPTS => {
                if !wait_for_retry(retry_delay(attempt), control).await {
                    return Err(cancelled());
                }
            }
            Err(error) => return Err(error.failure),
        }
    }
    unreachable!("the bounded pull attempt loop always returns or retries")
}

pub(super) fn retry_delay(attempt: usize) -> Duration {
    let exponent = attempt.saturating_sub(1).min(8) as u32;
    BASE_RETRY_DELAY
        .saturating_mul(1_u32 << exponent)
        .min(MAX_RETRY_DELAY)
}

async fn wait_for_retry(delay: Duration, control: &ImagePullControl) -> bool {
    if control.is_cancelled() {
        return false;
    }
    let sleep = tokio::time::sleep(delay);
    tokio::pin!(sleep);
    let mut cancellation_poll = tokio::time::interval(RETRY_CANCEL_POLL);
    cancellation_poll.tick().await;
    loop {
        tokio::select! {
            _ = &mut sleep => return !control.is_cancelled(),
            _ = cancellation_poll.tick() => {
                if control.is_cancelled() {
                    return false;
                }
            }
        }
    }
}

pub(super) fn is_retryable_pull_error(error: &bollard::errors::Error) -> bool {
    use bollard::errors::Error;

    match error {
        Error::RequestTimeoutError => true,
        Error::DockerResponseServerError {
            status_code,
            message,
        } => retryable_server_response(*status_code, Some(message)),
        Error::DockerStreamError { error } => retryable_stream_error(error),
        Error::IOError { err } => matches!(
            err.kind(),
            ErrorKind::TimedOut
                | ErrorKind::ConnectionReset
                | ErrorKind::ConnectionAborted
                | ErrorKind::BrokenPipe
                | ErrorKind::UnexpectedEof
                | ErrorKind::NotConnected
                | ErrorKind::ConnectionRefused
                | ErrorKind::NetworkUnreachable
                | ErrorKind::HostUnreachable
                | ErrorKind::NetworkDown
        ),
        Error::HyperResponseError { err } => {
            err.is_timeout() || err.is_incomplete_message() || err.is_closed()
        }
        Error::HyperLegacyError { err } => err.is_connect(),
        _ => false,
    }
}

pub(super) fn is_retryable_error_detail(code: Option<i64>, message: Option<&str>) -> bool {
    match code {
        Some(code) => u16::try_from(code)
            .ok()
            .is_some_and(|status| retryable_server_response(status, message)),
        None => message.is_some_and(retryable_stream_error),
    }
}

fn retryable_server_response(status: u16, message: Option<&str>) -> bool {
    matches!(status, 408 | 429 | 500 | 502 | 503 | 504)
        && !message.is_some_and(contains_permanent_failure_hint)
}

fn retryable_stream_error(message: &str) -> bool {
    if contains_permanent_failure_hint(message) || message.len() > MAX_ERROR_HINT_BYTES {
        return false;
    }
    if let Some(status) = explicit_status_code(message.as_bytes()) {
        return matches!(status, 408 | 429 | 500 | 502 | 503 | 504);
    }
    const TRANSIENT_HINTS: &[&str] = &[
        "timeout",
        "timed out",
        "connection reset",
        "connection refused",
        "connection aborted",
        "unexpected eof",
        "broken pipe",
        "temporarily unavailable",
        "temporary registry unavailable",
        "registry temporarily unavailable",
        "temporary failure",
        "service unavailable",
        "too many requests",
        "bad gateway",
        "gateway timeout",
        "internal server error",
        "http 408",
        "http 429",
        "http 500",
        "http 502",
        "http 503",
        "http 504",
        "status 408",
        "status 429",
        "status 500",
        "status 502",
        "status 503",
        "status 504",
        "status code 408",
        "status code 429",
        "status code 500",
        "status code 502",
        "status code 503",
        "status code 504",
    ];
    contains_any_ascii_case_insensitive(message.as_bytes(), TRANSIENT_HINTS)
}

fn explicit_status_code(message: &[u8]) -> Option<u16> {
    for marker in ["status code ", "http ", "status "] {
        let marker = marker.as_bytes();
        let mut start = 0;
        while let Some(offset) = find_ascii_case_insensitive(&message[start..], marker) {
            let marker_end = start + offset + marker.len();
            let Some(digit_start) = message[marker_end..]
                .iter()
                .position(|byte| !byte.is_ascii_whitespace())
                .map(|offset| marker_end + offset)
            else {
                break;
            };
            let Some(digits) = message.get(digit_start..digit_start + 3) else {
                break;
            };
            if digits.iter().all(u8::is_ascii_digit)
                && !message.get(digit_start + 3).is_some_and(u8::is_ascii_digit)
            {
                return Some(
                    u16::from(digits[0] - b'0') * 100
                        + u16::from(digits[1] - b'0') * 10
                        + u16::from(digits[2] - b'0'),
                );
            }
            start = marker_end;
        }
    }
    None
}

fn find_ascii_case_insensitive(message: &[u8], needle: &[u8]) -> Option<usize> {
    message
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle))
}

fn contains_permanent_failure_hint(message: &str) -> bool {
    const HINTS: &[&str] = &[
        "unauthorized",
        "authentication",
        "authorization",
        "denied",
        "forbidden",
        "401",
        "403",
        "digest",
        "checksum",
        "content hash",
        "platform",
        "manifest unknown",
        "no matching manifest",
        "unknown blob",
        "invalid",
        "malformed",
        "validation",
        "bad request",
        "unsupported",
        "not found",
    ];
    let message = message.as_bytes();
    if message.len() > MAX_ERROR_HINT_BYTES {
        return true;
    }
    contains_any_ascii_case_insensitive(message, HINTS)
}

fn contains_any_ascii_case_insensitive(message: &[u8], hints: &[&str]) -> bool {
    hints.iter().any(|hint| {
        let hint = hint.as_bytes();
        message
            .windows(hint.len())
            .any(|window| window.eq_ignore_ascii_case(hint))
    })
}
