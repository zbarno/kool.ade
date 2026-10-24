use std::time::Duration;

const CANCEL_MARKER_WAIT: Duration = Duration::from_secs(60);
const MARKER_PROBE_TIMEOUT: Duration = Duration::from_secs(3);

pub(super) async fn wait_for_cancel_marker(
    docker: &bollard::Docker,
    relay_container_id: &str,
    relay_runtime_user: &str,
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + CANCEL_MARKER_WAIT;
    while tokio::time::Instant::now() < deadline {
        let probe = tokio::time::timeout(
            MARKER_PROBE_TIMEOUT,
            super::super::probe::marker_is_active(
                docker,
                relay_container_id,
                relay_runtime_user,
                "/__cancel_started",
                "started",
            ),
        )
        .await;
        match probe {
            Ok(Ok(true)) => return Ok(()),
            Ok(Ok(false)) => {}
            Ok(Err(error)) => return Err(error),
            Err(_) => return Err("timed out probing the browser cancellation marker".into()),
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Err("browser never reached the delayed task page before cancellation".into())
}
