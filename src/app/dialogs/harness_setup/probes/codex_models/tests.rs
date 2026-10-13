use super::{CATALOG_TIMEOUT, discover, parse_model_page, query_catalog};
use serde_json::json;
use std::time::Instant;

#[test]
fn model_catalog_uses_only_visible_model_slugs_and_server_default() {
    let page = parse_model_page(&json!({
        "data": [
            {"model":"gpt-6.1-sol", "displayName":"GPT-6.1-Sol", "isDefault":true},
            {"id":"legacy-model-id", "displayName":"Legacy", "isDefault":false},
            {"displayName":"Invalid", "isDefault":false}
        ],
        "nextCursor":"next-page"
    }))
    .unwrap();
    assert_eq!(page.models, ["gpt-6.1-sol", "legacy-model-id"]);
    assert_eq!(page.default_model.as_deref(), Some("gpt-6.1-sol"));
    assert_eq!(page.next_cursor.as_deref(), Some("next-page"));
}

#[cfg(unix)]
#[test]
fn app_server_catalog_follows_cursors_and_deduplicates_model_slugs() {
    use std::{
        io::{BufRead, BufReader},
        process::{Command, Stdio},
        sync::mpsc,
        thread,
    };

    let script = r#"
read -r initialize
printf '%s\n' '{"id":1,"result":{}}'
read -r initialized
read -r first_page
case "$first_page" in *'"includeHidden":false'*'"limit":100'*) ;; *) exit 11 ;; esac
printf '%s\n' '{"id":2,"result":{"data":[{"model":"gpt-next","isDefault":true}],"nextCursor":"page-two"}}'
read -r second_page
case "$second_page" in *'"cursor":"page-two"'*) ;; *) exit 12 ;; esac
printf '%s\n' '{"id":3,"result":{"data":[{"id":"gpt-pro"},{"model":"gpt-next"}]}}'
"#;
    let mut child = Command::new("sh")
        .args(["-c", script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let output = child.stdout.take().unwrap();
    let (sender, messages) = mpsc::channel();
    let reader = thread::spawn(move || {
        for line in BufReader::new(output).lines() {
            let Ok(line) = line else { break };
            let Ok(message) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            if sender.send(Ok(message)).is_err() {
                break;
            }
        }
    });

    let catalog = query_catalog(
        &mut child,
        &mut input,
        &messages,
        Instant::now() + CATALOG_TIMEOUT,
    )
    .unwrap();

    drop(input);
    assert!(child.wait().unwrap().success());
    reader.join().unwrap();
    assert_eq!(catalog.0, ["gpt-next", "gpt-pro"]);
    assert_eq!(catalog.1.as_deref(), Some("gpt-next"));
}

#[cfg(unix)]
#[test]
fn cancelled_catalog_discovery_terminates_a_hung_app_server() {
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        thread,
        time::{Duration, Instant},
    };

    let path = std::env::temp_dir().join(format!("codex-model-probe-{}.sh", uuid::Uuid::new_v4()));
    fs::write(&path, "#!/bin/sh\nsleep 30\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    let cancellation = Arc::new(AtomicBool::new(false));
    let worker_cancellation = cancellation.clone();
    let worker_path = path.clone();
    let started = Instant::now();
    let worker = thread::spawn(move || {
        crate::harness::discovery::with_cancellation(worker_cancellation, || discover(&worker_path))
    });
    thread::sleep(Duration::from_millis(80));
    cancellation.store(true, Ordering::Release);
    let result = worker.join().unwrap();
    assert!(result.unwrap_err().to_string().contains("cancelled"));
    assert!(started.elapsed() < Duration::from_secs(2));
    fs::remove_file(path).unwrap();
}

#[cfg(unix)]
fn fake_app_server(body: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    use std::os::unix::fs::PermissionsExt;

    let root = std::env::temp_dir().join(format!("codex-model-server-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let binary = root.join("codex");
    std::fs::write(&binary, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    (root, binary)
}

#[cfg(unix)]
fn assert_catalog_error_is_bounded(binary: std::path::PathBuf, expected: &str) {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
            mpsc,
        },
        thread,
        time::Duration,
    };

    let (tx, rx) = mpsc::channel();
    let cancellation = Arc::new(AtomicBool::new(false));
    let worker_cancellation = cancellation.clone();
    thread::spawn(move || {
        let _ = tx.send(crate::harness::discovery::with_cancellation(
            worker_cancellation,
            || discover(&binary),
        ));
    });
    let result = match rx.recv_timeout(Duration::from_secs(3)) {
        Ok(result) => result.unwrap_err(),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            cancellation.store(true, Ordering::Release);
            rx.recv_timeout(Duration::from_secs(2))
                .expect("cancelled invalid app-server probe must exit promptly")
                .unwrap_err()
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => panic!("probe worker stopped unexpectedly"),
    };
    assert!(result.to_string().contains(expected), "{result:#}");
}

#[cfg(unix)]
#[test]
fn app_server_malformed_json_is_a_bounded_catalog_failure() {
    let (root, binary) = fake_app_server("read -r initialize\nprintf '%s\\n' 'not-json'");
    assert_catalog_error_is_bounded(binary, "malformed JSON");
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn app_server_wrong_response_id_is_rejected_without_waiting_for_timeout() {
    let (root, binary) =
        fake_app_server("read -r initialize\nprintf '%s\\n' '{\"id\":99,\"result\":{}}'\nsleep 30");
    assert_catalog_error_is_bounded(binary, "unexpected response ID");
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn app_server_oversized_frame_is_rejected_without_waiting_for_timeout() {
    let (root, binary) = fake_app_server("read -r initialize\nprintf '%65537s\\n' x");
    assert_catalog_error_is_bounded(binary, "frame exceeded the size limit");
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn app_server_repeated_model_cursor_is_rejected_without_an_unbounded_page_loop() {
    let (root, binary) = fake_app_server(
        r#"
read -r initialize
printf '%s\n' '{"id":1,"result":{}}'
read -r initialized
read -r first_page
printf '%s\n' '{"id":2,"result":{"data":[{"model":"model-a"}],"nextCursor":"same"}}'
read -r second_page
printf '%s\n' '{"id":3,"result":{"data":[{"model":"model-b"}],"nextCursor":"same"}}'
"#,
    );
    assert_catalog_error_is_bounded(binary, "repeated a cursor");
    std::fs::remove_dir_all(root).unwrap();
}
