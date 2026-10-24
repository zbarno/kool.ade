use super::*;

#[test]
fn runtime_qa_screenshot_protocol_returns_only_allowlisted_image_and_records_view() {
    // Production boundary: JSON-RPC tools/list and tools/call through the app MCP server.
    // A unit test of the image reader cannot prove that providers receive MCP image blocks
    // or that the app records a successful screenshot request. No external services are
    // needed; tempdir setup is low cost.
    use sha2::Digest;

    let directory = tempfile::tempdir().unwrap();
    let image_root = directory.path().join("qa-screenshots");
    let audit_root = directory.path().join("qa-screenshot-views");
    std::fs::create_dir(&image_root).unwrap();
    std::fs::create_dir(&audit_root).unwrap();
    let screenshot_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let mut image_bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image::RgbaImage::new(1, 1))
        .write_to(&mut image_bytes, image::ImageFormat::Png)
        .unwrap();
    let image_bytes = image_bytes.into_inner();
    std::fs::write(
        image_root.join(format!("{screenshot_id}.png")),
        &image_bytes,
    )
    .unwrap();
    let tampered_id = uuid::Uuid::new_v4().hyphenated().to_string();
    std::fs::write(image_root.join(format!("{tampered_id}.png")), b"not a PNG").unwrap();
    let image_sha = format!("{:x}", sha2::Sha256::digest(&image_bytes));
    let config = super::super::ServerConfig::runtime_qa_screenshot_review(
        "00000000-0000-4000-8000-000000000001".into(),
        "00000000-0000-4000-8000-000000000002".into(),
        format!(
            "00000000-0000-4000-8000-000000000003:00000000-0000-4000-8000-000000000002:{}",
            "a".repeat(40)
        ),
        "00000000-0000-4000-8000-000000000004".into(),
        image_root,
        audit_root.clone(),
        [
            (screenshot_id.clone(), image_sha),
            (
                tampered_id.clone(),
                format!("{:x}", sha2::Sha256::digest(&image_bytes)),
            ),
        ]
        .into_iter()
        .collect(),
    );
    config.validate().unwrap();
    assert_eq!(
        super::super::tools::list(&config)[0]["name"],
        "koolade_runtime_qa_screenshot"
    );

    let screenshot_request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "koolade_runtime_qa_screenshot",
            "arguments": { "screenshot_id": screenshot_id },
        },
    });
    let shell_request = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "koolade_bash",
            "arguments": { "command": "cat README.md" },
        },
    });
    let tampered_request = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "koolade_runtime_qa_screenshot",
            "arguments": { "screenshot_id": tampered_id },
        },
    });
    let mut output = Vec::new();
    serve_stream(
        Path::new("/tmp/mcp/server.json"),
        &config,
        &mut io::Cursor::new(format!(
            "{screenshot_request}\n{shell_request}\n{tampered_request}\n"
        )),
        &mut output,
    )
    .unwrap();
    let responses = output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(responses.len(), 3);
    let response = &responses[0];
    let content = response["result"]["content"].as_array().unwrap();
    assert_eq!(content[1]["type"], "image");
    assert_eq!(content[1]["mimeType"], "image/png");
    let decoded = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        content[1]["data"].as_str().unwrap(),
    )
    .unwrap();
    assert_eq!(decoded, image_bytes);
    assert!(audit_root.join(format!("{screenshot_id}.viewed")).is_file());
    assert_eq!(responses[1]["result"]["isError"], true);
    assert_eq!(responses[2]["result"]["isError"], true);
    assert!(!audit_root.join(format!("{tampered_id}.viewed")).exists());
}

#[test]
fn failed_screenshot_response_flush_does_not_record_an_inspection() {
    // Production boundary: MCP audit state advances only after response flush.
    // A screenshot-tool unit test cannot cover protocol delivery failure; this
    // uses a synthetic PNG and failing in-memory writer, with no external
    // services and low setup cost.
    use std::io::{self, Write};

    struct FlushFailure(Vec<u8>);
    impl Write for FlushFailure {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::other("synthetic MCP delivery failure"))
        }
    }

    use sha2::Digest;

    let directory = tempfile::tempdir().unwrap();
    let image_root = directory.path().join("qa-screenshots");
    let audit_root = directory.path().join("qa-screenshot-views");
    std::fs::create_dir(&image_root).unwrap();
    std::fs::create_dir(&audit_root).unwrap();
    let screenshot_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image::RgbaImage::new(1, 1))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let png = png.into_inner();
    std::fs::write(image_root.join(format!("{screenshot_id}.png")), &png).unwrap();
    let config = super::super::ServerConfig::runtime_qa_screenshot_review(
        "00000000-0000-4000-8000-000000000001".into(),
        "00000000-0000-4000-8000-000000000002".into(),
        format!(
            "00000000-0000-4000-8000-000000000003:00000000-0000-4000-8000-000000000002:{}",
            "a".repeat(40)
        ),
        "00000000-0000-4000-8000-000000000004".into(),
        image_root,
        audit_root.clone(),
        [(
            screenshot_id.clone(),
            format!("{:x}", sha2::Sha256::digest(&png)),
        )]
        .into_iter()
        .collect(),
    );
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "koolade_runtime_qa_screenshot",
            "arguments": { "screenshot_id": screenshot_id },
        },
    });
    let error = serve_stream(
        Path::new("/tmp/mcp/server.json"),
        &config,
        &mut io::Cursor::new(format!("{request}\n")),
        &mut FlushFailure(Vec::new()),
    )
    .unwrap_err();

    assert!(error.to_string().contains("synthetic MCP delivery failure"));
    assert!(!audit_root.join(format!("{screenshot_id}.viewed")).exists());
}

/// Production boundary: MCP JSON-RPC exposes no tools for a screenshot-free
/// headless review and rejects forged image IDs. A config unit test alone
/// cannot prove providers see an empty tool list. Uses temp files only; no
/// Docker, SSH, browser, or network, with low setup cost.
#[test]
fn screenshot_free_headless_review_exposes_no_tools_or_fake_images() {
    let directory = tempfile::tempdir().unwrap();
    let image_root = directory.path().join("qa-screenshots");
    let audit_root = directory.path().join("qa-screenshot-views");
    std::fs::create_dir(&image_root).unwrap();
    std::fs::create_dir(&audit_root).unwrap();
    let config = super::super::ServerConfig::runtime_qa_screenshot_review(
        "00000000-0000-4000-8000-000000000001".into(),
        "00000000-0000-4000-8000-000000000002".into(),
        format!(
            "00000000-0000-4000-8000-000000000003:00000000-0000-4000-8000-000000000002:{}",
            "a".repeat(40)
        ),
        "00000000-0000-4000-8000-000000000004".into(),
        image_root,
        audit_root,
        std::collections::BTreeMap::new(),
    );
    config.validate().unwrap();
    let list_request = json!({"jsonrpc":"2.0","id":1,"method":"tools/list"});
    let fake_image_request = json!({
        "jsonrpc":"2.0","id":2,"method":"tools/call",
        "params":{"name":"koolade_runtime_qa_screenshot","arguments":{"screenshot_id":"00000000-0000-4000-8000-000000000005"}}
    });
    let mut output = Vec::new();

    serve_stream(
        Path::new("/tmp/mcp/server.json"),
        &config,
        &mut io::Cursor::new(format!("{list_request}\n{fake_image_request}\n")),
        &mut output,
    )
    .unwrap();

    let responses = output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(responses.len(), 2);
    assert!(
        responses[0]["result"]["tools"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(responses[1]["result"]["isError"], true);
}
