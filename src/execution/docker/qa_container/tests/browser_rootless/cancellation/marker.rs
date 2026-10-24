use super::*;
use futures_util::StreamExt;
use std::io::Read;

const CANCEL_MARKER: &str = "/tmp/qa-browser-cancel-started";
const CANCEL_MARKER_WAIT: Duration = Duration::from_secs(60);

pub(super) async fn wait_for_cancel_marker(
    docker: &bollard::Docker,
    task_container_id: &str,
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + CANCEL_MARKER_WAIT;
    while tokio::time::Instant::now() < deadline {
        let marker_read = tokio::time::timeout(Duration::from_secs(4), async {
            let options = bollard::query_parameters::DownloadFromContainerOptionsBuilder::default()
                .path(CANCEL_MARKER)
                .build();
            let mut archive_stream =
                docker.download_from_container(task_container_id, Some(options));
            let mut bytes = Vec::new();
            let mut complete = true;
            while let Some(chunk) = archive_stream.next().await {
                match chunk {
                    Ok(chunk) => bytes.extend_from_slice(&chunk),
                    Err(_) => {
                        complete = false;
                        break;
                    }
                }
            }
            complete && archive_contains_cancel_marker(&bytes)
        })
        .await;
        match marker_read {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(_) => return Err("timed out reading browser cancellation marker".into()),
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err("browser never reached the delayed task page before cancellation".into())
}

fn archive_contains_cancel_marker(bytes: &[u8]) -> bool {
    let mut archive = tar::Archive::new(bytes);
    let Ok(entries) = archive.entries() else {
        return false;
    };
    entries.flatten().any(|mut entry| {
        let is_marker = entry
            .path()
            .map(|path| {
                path.file_name().and_then(std::ffi::OsStr::to_str)
                    == Some("qa-browser-cancel-started")
            })
            .unwrap_or(false);
        if !is_marker {
            return false;
        }
        let mut content = String::new();
        entry.read_to_string(&mut content).is_ok() && content == "started"
    })
}
