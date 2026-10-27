use super::*;

#[test]
fn exports_only_one_active_saved_api_key_for_the_requested_integration() {
    let output = br#"[{"id":"cred_1","integrationID":"anthropic","active":true,"value":{"type":"key","key":"synthetic-api-key"}}]"#;
    match parse_export(output, "anthropic") {
        AuthExport::ApiKey(key) => assert_eq!(&*key, "synthetic-api-key"),
        _ => panic!("the active API key was not selected"),
    }
}

#[test]
fn empty_export_is_missing_and_ambiguous_or_oauth_exports_stay_unsupported() {
    assert!(matches!(
        parse_export(b"[]", "anthropic"),
        AuthExport::NoSavedCredential
    ));
    let outputs: [&[u8]; 4] = [
        b"not-json",
        br#"[{"integrationID":"openai","active":true,"value":{"type":"key","key":"synthetic-api-key"}}]"#,
        br#"[{"integrationID":"anthropic","active":true,"value":{"type":"key","key":"first-synthetic-key"}},{"integrationID":"anthropic","active":true,"value":{"type":"key","key":"second-synthetic-key"}}]"#,
        br#"[{"integrationID":"anthropic","active":true,"value":{"type":"oauth","access":"synthetic-access","refresh":"synthetic-refresh","expires":1}}]"#,
    ];
    for output in outputs {
        assert!(matches!(
            parse_export(output, "anthropic"),
            AuthExport::Unsupported
        ));
    }
}

#[cfg(unix)]
#[test]
fn provider_scoped_export_crosses_the_bounded_private_subprocess_boundary() {
    use std::os::unix::fs::PermissionsExt;

    // This crosses the actual process-capture boundary; parser tests alone cannot prove the
    // command, timeout, environment scrub, and bounded stdout path. It needs only /bin/sh and a
    // temporary executable, with no provider account, network, Docker, or SSH.
    let root =
        std::env::temp_dir().join(format!("koolade-opencode-export-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let binary = root.join("opencode");
    std::fs::write(
        &binary,
        r#"#!/bin/sh
test "$*" = "--pure auth export anthropic" || exit 8
printf '%s\n' '[{"integrationID":"anthropic","active":true,"value":{"type":"key","key":"synthetic-api-key"}}]'
printf '%s\n' 'synthetic-stderr-secret' >&2
"#,
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();

    match export_api_key(&binary, "anthropic") {
        AuthExport::ApiKey(key) => assert_eq!(&*key, "synthetic-api-key"),
        _ => panic!("provider-scoped export did not return the selected key"),
    }

    std::fs::remove_dir_all(root).unwrap();
}
