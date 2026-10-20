use super::*;

#[test]
fn discovery_records_are_provider_allowlisted_and_do_not_include_probe_data() {
    let encoded = serde_json::to_string(&ProbeRecord {
        schema_version: 1,
        scan_id: 7,
        provider: "codex",
        stage: "provider_probe",
        duration_ms: 10,
        outcome: "failed",
        failure_code: Some("provider_unavailable"),
    })
    .unwrap();
    assert!(!encoded.contains("token"));
    assert!(!encoded.contains("/home/"));
    assert!(supported_provider("codex"));
    assert!(!supported_provider("unknown"));
    assert_eq!(supported_stage(Some("models")), Some("models"));
    assert_eq!(supported_stage(Some("private path")), None);
    assert_eq!(
        supported_failure_code("authentication_required"),
        Some("authentication_required")
    );
    assert_eq!(
        supported_failure_code("internal_probe_failure"),
        Some("internal_probe_failure")
    );
    assert_eq!(
        supported_failure_code("configuration_required"),
        Some("configuration_required")
    );
    assert_eq!(supported_failure_code("token=secret"), None);
    assert_eq!(
        logged_failure_code(true, Some("model_catalog_unavailable")),
        Some("model_catalog_unavailable")
    );
    assert_eq!(logged_failure_code(true, None), None);
    assert_eq!(
        logged_failure_code(false, None),
        Some("provider_unavailable")
    );
    let typed = ProbeRecord {
        schema_version: 1,
        scan_id: 8,
        provider: "codex",
        stage: supported_stage(Some("models")).unwrap(),
        duration_ms: 18,
        outcome: "failed",
        failure_code: supported_failure_code("model_catalog_unavailable"),
    };
    let encoded = serde_json::to_string(&typed).unwrap();
    assert!(encoded.contains("\"stage\":\"models\""));
    assert!(encoded.contains("\"failure_code\":\"model_catalog_unavailable\""));
}

#[test]
fn retention_keeps_complete_recent_lines_within_the_size_limit() {
    let mut previous = vec![b'x'; MAX_LOG_BYTES];
    previous.extend_from_slice(b"\nold\n");
    let retained = retain_bounded(&previous, b"new\n");
    assert!(retained.len() <= MAX_LOG_BYTES);
    assert!(retained.ends_with(b"new\n"));
    assert_eq!(retained, b"old\nnew\n");
}

/// Production boundary: one discovery attempt is serialized to the operator-local diagnostics
/// file with private permissions. Unit tests of the record type cannot prove the selected path,
/// filesystem modes, and on-disk sanitization together; it uses only a temporary directory and
/// two tiny writes, with no external services.
#[test]
fn probe_attempt_is_written_to_local_diagnostics_without_probe_data() {
    let root = std::env::temp_dir().join(format!(
        "koolade-discovery-log-{}",
        uuid::Uuid::new_v4().hyphenated()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("KOOLADE_HOME", &root);

    record_probe_result(
        42,
        "codex",
        27,
        false,
        Some("models"),
        Some("model_catalog_unavailable"),
    );
    record_probe_result(
        43,
        "codex",
        29,
        false,
        Some("/private/synthetic-path"),
        Some("token=synthetic-secret"),
    );
    record_probe_result(44, "unsupported-provider", 31, false, Some("models"), None);

    let path = root.join("diagnostics").join(LOG_NAME);
    let contents = std::fs::read_to_string(&path).unwrap();
    let mut records = contents
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap());
    let record = records.next().unwrap();
    let fallback = records.next().unwrap();
    assert!(records.next().is_none());
    assert_eq!(record["schema_version"], 1);
    assert_eq!(record["scan_id"], 42);
    assert_eq!(record["provider"], "codex");
    assert_eq!(record["duration_ms"], 27);
    assert_eq!(record["stage"], "models");
    assert_eq!(record["failure_code"], "model_catalog_unavailable");
    assert_eq!(record.as_object().unwrap().len(), 7);
    assert_eq!(fallback["schema_version"], 1);
    assert_eq!(fallback["scan_id"], 43);
    assert_eq!(fallback["outcome"], "failed");
    assert_eq!(fallback["stage"], "provider_probe");
    assert_eq!(fallback["failure_code"], "provider_unavailable");
    assert!(!contents.contains("synthetic-secret"));
    assert!(!contents.contains("/private/synthetic-path"));
    assert!(!contents.contains("unsupported-provider"));
    assert!(contents.len() <= 1024);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    drop(environment);
    std::fs::remove_dir_all(root).unwrap();
}
