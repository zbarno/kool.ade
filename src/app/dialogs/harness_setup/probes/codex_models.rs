use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
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
    for name in crate::harness::execution_security::CliProvider::Codex.excluded_child_environment()
    {
        command.env_remove(name);
    }
    let mut child = command.spawn()?;
    let mut input = child
        .stdin
        .take()
        .ok_or_else(|| anyhow::anyhow!("Codex app-server did not expose its input stream"))?;
    let output = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("Codex app-server did not expose its output stream"))?;
    let (messages_tx, messages_rx) = mpsc::channel();
    let reader = thread::spawn(move || {
        for line in BufReader::new(output).lines() {
            let Ok(line) = line else { break };
            if let Ok(message) = serde_json::from_str::<Value>(&line)
                && messages_tx.send(message).is_err()
            {
                break;
            }
        }
    });

    let result = query_catalog(&mut child, &mut input, &messages_rx);
    drop(input);
    let _ = child.kill();
    let _ = child.wait();
    let _ = reader.join();
    result
}

fn query_catalog(
    child: &mut Child,
    input: &mut impl Write,
    messages: &mpsc::Receiver<Value>,
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
    receive_response(child, messages, 1)?;
    write_message(input, &json!({"method":"initialized", "params":{}}))?;

    let mut all_models = Vec::new();
    let mut default_model = None;
    let mut cursor: Option<String> = None;
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
        let response = receive_response(child, messages, request_id)?;
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
    messages: &mpsc::Receiver<Value>,
    request_id: u64,
) -> anyhow::Result<Value> {
    let deadline = Instant::now() + REQUEST_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        anyhow::ensure!(
            !remaining.is_zero(),
            "Codex app-server did not return a model discovery response in time"
        );
        match messages.recv_timeout(remaining.min(Duration::from_millis(200))) {
            Ok(message) if message["id"].as_u64() == Some(request_id) => {
                anyhow::ensure!(
                    message.get("error").is_none(),
                    "Codex app-server rejected model discovery"
                );
                return Ok(message);
            }
            Ok(_) => {}
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
    for model in data {
        let Some(slug) = model
            .get("model")
            .or_else(|| model.get("id"))
            .and_then(Value::as_str)
            .filter(|slug| !slug.trim().is_empty())
        else {
            continue;
        };
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

#[cfg(test)]
mod tests {
    use super::{parse_model_page, query_catalog};
    use serde_json::json;

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
                if sender.send(message).is_err() {
                    break;
                }
            }
        });

        let catalog = query_catalog(&mut child, &mut input, &messages).unwrap();

        drop(input);
        assert!(child.wait().unwrap().success());
        reader.join().unwrap();
        assert_eq!(catalog.0, ["gpt-next", "gpt-pro"]);
        assert_eq!(catalog.1.as_deref(), Some("gpt-next"));
    }
}
