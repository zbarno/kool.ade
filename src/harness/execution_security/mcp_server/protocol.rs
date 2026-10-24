use super::{ServerConfig, tools};
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::Path;

const MESSAGE_LIMIT: usize = 1_000_000;

pub(super) fn serve(config_path: &Path, config: &ServerConfig) -> anyhow::Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();
    serve_stream(config_path, config, &mut input, &mut output)
}

fn serve_stream(
    config_path: &Path,
    config: &ServerConfig,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> anyhow::Result<()> {
    let mut line = Vec::new();
    loop {
        if !read_bounded_line(input, &mut line)? {
            return Ok(());
        }
        let message = match serde_json::from_slice::<Value>(&line) {
            Ok(message) => message,
            Err(error) => {
                write_message(
                    output,
                    json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": format!("Invalid JSON-RPC message: {error}") } }),
                )?;
                continue;
            }
        };
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            write_error(
                output,
                message.get("id"),
                -32600,
                "Invalid JSON-RPC request",
            )?;
            continue;
        };
        let Some(id) = message.get("id") else {
            if method == "exit" {
                return Ok(());
            }
            continue;
        };
        let params = &message["params"];
        let mut post_delivery_audit = None;
        let result = match method {
            "initialize" => Ok(json!({
                "protocolVersion": params["protocolVersion"].as_str().unwrap_or("2024-11-05"),
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "koolade-application-boundary", "version": env!("CARGO_PKG_VERSION") },
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools::list(config) })),
            "tools/call" => {
                let call = tools::call_with_audit(
                    config_path,
                    config,
                    params["name"].as_str().unwrap_or_default(),
                    params.get("arguments").unwrap_or(&Value::Null),
                );
                post_delivery_audit = call.after_delivery;
                Ok(call.response)
            }
            "shutdown" => Ok(json!({})),
            other => Err((-32601, format!("Unsupported MCP method: {other}"))),
        };
        match result {
            Ok(result) => {
                write_message(
                    output,
                    json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                )?;
                if let Some(audit) = post_delivery_audit {
                    audit.commit_after_delivery();
                }
            }
            Err((code, message)) => write_error(output, Some(id), code, &message)?,
        }
    }
}

fn read_bounded_line(input: &mut impl BufRead, line: &mut Vec<u8>) -> anyhow::Result<bool> {
    line.clear();
    loop {
        let available = input.fill_buf()?;
        if available.is_empty() {
            return Ok(!line.is_empty());
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let count = newline.map_or(available.len(), |index| index + 1);
        anyhow::ensure!(
            line.len() + count <= MESSAGE_LIMIT,
            "MCP request exceeds the size limit"
        );
        line.extend_from_slice(&available[..count]);
        input.consume(count);
        if newline.is_some() {
            return Ok(true);
        }
    }
}

fn write_error(
    output: &mut impl Write,
    id: Option<&Value>,
    code: i32,
    message: &str,
) -> anyhow::Result<()> {
    write_message(
        output,
        json!({ "jsonrpc": "2.0", "id": id.cloned().unwrap_or(Value::Null), "error": { "code": code, "message": message } }),
    )
}

fn write_message(output: &mut impl Write, message: Value) -> anyhow::Result<()> {
    serde_json::to_writer(&mut *output, &message)?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(())
}

#[cfg(test)]
#[path = "protocol/tests.rs"]
mod tests;
