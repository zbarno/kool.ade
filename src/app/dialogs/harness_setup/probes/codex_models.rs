use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

const CATALOG_TIMEOUT: Duration = crate::harness::discovery::MODEL_PROBE_TIMEOUT;
const MAX_FRAME_BYTES: usize = 64 * 1024;
const MAX_CATALOG_BYTES: usize = crate::harness::discovery::MAX_PROBE_OUTPUT_BYTES;
const PAGE_SIZE: u64 = 100;
const MAX_PAGES: usize = 10;

#[derive(Default)]
struct ModelPage {
    models: Vec<String>,
    default_model: Option<String>,
    next_cursor: Option<String>,
}

pub(super) fn discover(binary: &Path) -> anyhow::Result<(Vec<String>, Option<String>)> {
    let mut command = Command::new(binary);
    command
        .args(["app-server", "--listen", "stdio://"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let directory =
        crate::harness::execution_security::PrivateDirectory::create("koolade-codex-models")?;
    command.current_dir(&directory.path);
    crate::harness::discovery::sanitize_probe_environment(&mut command);
    for name in crate::harness::execution_security::CliProvider::Codex.excluded_child_environment()
    {
        command.env_remove(name);
    }
    for name in [
        "NODE_OPTIONS",
        "NODE_PATH",
        "BASH_ENV",
        "ENV",
        "PYTHONPATH",
        "PYTHONHOME",
    ] {
        command.env_remove(name);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_SUSPENDED);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn()?;
    #[cfg(windows)]
    let process_tree =
        match crate::harness::discovery::windows::WindowsProcessTree::attach(child.id()) {
            Ok(tree) => Some(tree),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                anyhow::bail!("could not supervise Codex model discovery process tree: {error}");
            }
        };
    let mut input = child
        .stdin
        .take()
        .ok_or_else(|| anyhow::anyhow!("Codex app-server did not expose its input stream"))?;
    let output = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("Codex app-server did not expose its output stream"))?;
    let (messages_tx, messages_rx) = mpsc::sync_channel(256);
    let bytes_read = std::sync::Arc::new(AtomicUsize::new(0));
    let reader_bytes = bytes_read.clone();
    let (reader_done_tx, reader_done_rx) = mpsc::channel();
    let reader = thread::spawn(move || {
        let mut reader = BufReader::new(output);
        loop {
            let mut frame = Vec::new();
            let mut limited = (&mut reader).take((MAX_FRAME_BYTES + 1) as u64);
            let read = match limited.read_until(b'\n', &mut frame) {
                Ok(read) => read,
                Err(_) => break,
            };
            if read == 0 {
                break;
            }
            if frame.len() > MAX_FRAME_BYTES {
                let _ = messages_tx.send(Err(
                    "Codex app-server frame exceeded the size limit".to_owned()
                ));
                break;
            }
            if reader_bytes
                .fetch_add(read, Ordering::Relaxed)
                .saturating_add(read)
                > MAX_CATALOG_BYTES
            {
                let _ = messages_tx.send(Err(
                    "Codex app-server output exceeded the size limit".to_owned()
                ));
                break;
            }
            let frame = frame.strip_suffix(b"\n").unwrap_or(&frame);
            let parsed = serde_json::from_slice::<Value>(frame)
                .map_err(|_| "Codex app-server returned malformed JSON".to_owned());
            if messages_tx.send(parsed).is_err() {
                break;
            }
        }
        let _ = reader_done_tx.send(());
    });

    let result = query_catalog(
        &mut child,
        &mut input,
        &messages_rx,
        Instant::now() + CATALOG_TIMEOUT,
    );
    drop(input);
    terminate_group(child.id());
    let _ = child.kill();
    let _ = child.wait();
    #[cfg(windows)]
    drop(process_tree);
    if reader_done_rx
        .recv_timeout(Duration::from_millis(500))
        .is_ok()
    {
        let _ = reader.join();
    } else {
        drop(reader);
    }
    result
}

fn query_catalog(
    child: &mut Child,
    input: &mut impl Write,
    messages: &mpsc::Receiver<Result<Value, String>>,
    deadline: Instant,
) -> anyhow::Result<(Vec<String>, Option<String>)> {
    write_message(
        input,
        &json!({
            "id": 1,
            "method": "initialize",
            "params": {
                "clientInfo": {"name":"koolade", "title":"Kool.ad/e", "version": env!("CARGO_PKG_VERSION")},
                "capabilities": {}
            }
        }),
    )?;
    receive_response(child, messages, 1, deadline)?;
    write_message(input, &json!({"method":"initialized", "params":{}}))?;

    let mut all_models = Vec::new();
    let mut default_model = None;
    let mut cursor: Option<String> = None;
    let mut seen_cursors = BTreeSet::new();
    for page_index in 0..MAX_PAGES {
        let request_id = (page_index + 2) as u64;
        let mut params = json!({"includeHidden":false, "limit":PAGE_SIZE});
        if let Some(cursor) = cursor.as_deref() {
            params["cursor"] = Value::String(cursor.to_owned());
        }
        write_message(
            input,
            &json!({"id":request_id, "method":"model/list", "params":params}),
        )?;
        let response = receive_response(child, messages, request_id, deadline)?;
        let page = parse_model_page(&response["result"])?;
        for model in page.models {
            if !all_models.contains(&model) {
                all_models.push(model);
            }
        }
        default_model = default_model.or(page.default_model);
        cursor = page.next_cursor;
        if cursor.is_none() {
            return Ok((all_models, default_model));
        }
        let next_cursor = cursor.as_deref().unwrap_or_default();
        anyhow::ensure!(
            next_cursor.len() <= 1024,
            "Codex model cursor exceeded the size limit"
        );
        anyhow::ensure!(
            seen_cursors.insert(next_cursor.to_owned()),
            "Codex model catalog repeated a cursor"
        );
    }
    anyhow::bail!("Codex model catalog exceeded the supported pagination limit")
}

fn write_message(input: &mut impl Write, message: &Value) -> anyhow::Result<()> {
    serde_json::to_writer(&mut *input, message)?;
    input.write_all(b"\n")?;
    input.flush()?;
    Ok(())
}

fn receive_response(
    child: &mut Child,
    messages: &mpsc::Receiver<Result<Value, String>>,
    request_id: u64,
    deadline: Instant,
) -> anyhow::Result<Value> {
    loop {
        anyhow::ensure!(
            !crate::harness::discovery::cancellation_requested(),
            "Codex app-server discovery was cancelled"
        );
        let remaining = deadline.saturating_duration_since(Instant::now());
        anyhow::ensure!(
            !remaining.is_zero(),
            "Codex app-server did not return a model discovery response in time"
        );
        match messages.recv_timeout(remaining.min(Duration::from_millis(200))) {
            Ok(Err(error)) => anyhow::bail!("{error}"),
            Ok(Ok(message)) if message["id"].as_u64() == Some(request_id) => {
                anyhow::ensure!(
                    message.get("error").is_none(),
                    "Codex app-server rejected model discovery"
                );
                return Ok(message);
            }
            Ok(Ok(message)) if message.get("id").is_some() => {
                anyhow::bail!("Codex app-server returned an unexpected response ID")
            }
            Ok(Ok(_)) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {
                anyhow::ensure!(
                    child.try_wait()?.is_none(),
                    "Codex app-server exited during model discovery"
                );
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                anyhow::bail!("Codex app-server closed during model discovery")
            }
        }
    }
}

fn parse_model_page(value: &Value) -> anyhow::Result<ModelPage> {
    let data = value
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("Codex returned an invalid model catalog"))?;
    let mut page = ModelPage::default();
    anyhow::ensure!(
        data.len() <= PAGE_SIZE as usize,
        "Codex model page exceeded the size limit"
    );
    for model in data {
        let Some(slug) = model
            .get("model")
            .or_else(|| model.get("id"))
            .and_then(Value::as_str)
            .filter(|slug| !slug.trim().is_empty())
        else {
            continue;
        };
        anyhow::ensure!(
            slug.len() <= 256 && !slug.chars().any(char::is_control),
            "Codex model identifier is invalid"
        );
        page.models.push(slug.to_owned());
        if model.get("isDefault").and_then(Value::as_bool) == Some(true) {
            page.default_model = Some(slug.to_owned());
        }
    }
    page.next_cursor = value
        .get("nextCursor")
        .and_then(Value::as_str)
        .filter(|cursor| !cursor.is_empty())
        .map(str::to_owned);
    Ok(page)
}

mod terminate;
use terminate::terminate_group;

#[cfg(test)]
#[path = "codex_models/tests.rs"]
mod tests;
