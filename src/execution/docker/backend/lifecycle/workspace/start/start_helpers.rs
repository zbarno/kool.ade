use crate::execution::errors::{ExecutionError, ExecutionErrorKind};
use std::time::Duration;

pub(super) fn engine_operation_error(
    operation: &str,
    error: bollard::errors::Error,
) -> ExecutionError {
    let details = match error {
        bollard::errors::Error::DockerResponseServerError {
            status_code,
            message,
        } => format!(
            "Docker API returned HTTP {status_code}: {}",
            safe_detail(&message)
        ),
        _ => format!("Docker Engine {operation} request failed"),
    };
    ExecutionError::new(
        ExecutionErrorKind::WorkerDisconnected,
        format!("{operation} failed: {details}"),
        true,
    )
}

pub(super) fn deadline_instant(deadline_unix_ms: u64) -> tokio::time::Instant {
    tokio::time::Instant::now()
        + Duration::from_millis(deadline_unix_ms.saturating_sub(super::now_unix_ms()))
}

pub(super) fn deadline_error() -> ExecutionError {
    ExecutionError::new(
        ExecutionErrorKind::DeadlineExceeded,
        "Execution deadline elapsed while preparing the Docker workspace",
        false,
    )
}

fn safe_detail(message: &str) -> String {
    let mut result = String::new();
    for word in message.split_whitespace() {
        if !result.is_empty() {
            result.push(' ');
        }
        if word.starts_with('/') {
            result.push_str("<path>");
        } else {
            result.push_str(word);
        }
        if result.chars().count() >= 256 {
            break;
        }
    }
    result
}
