use super::*;

#[test]
fn configured_credentials_do_not_prove_current_account_access() {
    let output = "┌  Credentials /synthetic/private/auth.json
│
● Anthropic oauth
│
└  1 credential
┌  Environment
│
● Groq GROQ_API_KEY
│
└  1 environment variable
";
    assert_eq!(parse_auth_list(output.as_bytes()), ReadinessState::Unknown);
}

#[test]
fn explicit_empty_auth_summary_is_unavailable() {
    let output = "┌  Credentials /synthetic/auth.json\n└  0 credentials\n┌  Environment\n└  0 environment variables\n";
    assert_eq!(
        parse_auth_list(output.as_bytes()),
        ReadinessState::Unavailable
    );
}

#[test]
fn ambiguous_or_changed_auth_output_remains_unknown() {
    let invalid_utf8 = [0xff];
    let outputs: [&[u8]; 4] = [
        b"OpenCode auth providers: none",
        "└ 0 credentials\n└ 2 credentials\n".as_bytes(),
        &invalid_utf8,
        "└ 0 credentials\n┌  Environment\n".as_bytes(),
    ];
    for output in outputs {
        assert_eq!(parse_auth_list(output), ReadinessState::Unknown,);
    }
}

#[test]
fn credential_counts_without_both_sections_are_unknown() {
    for output in ["└ 1 credential\n", "└ 0 credentials\n"] {
        assert_eq!(
            parse_auth_list(output.as_bytes()),
            ReadinessState::Unknown,
            "partial summary became authoritative: {output:?}"
        );
    }
}

#[test]
fn summary_counts_must_match_listed_provider_and_environment_rows() {
    let outputs = [
        "┌  Credentials ~/.local/share/opencode/auth.json\n└  1 credential\n┌  Environment\n└  0 environment variables\n",
        "┌  Credentials ~/.local/share/opencode/auth.json\n● Anthropic oauth\n└  0 credentials\n┌  Environment\n└  0 environment variables\n",
        "┌  Credentials ~/.local/share/opencode/auth.json\n└  0 credentials\n┌  Environment\n● Groq GROQ_API_KEY\n└  0 environment variables\n",
    ];
    for output in outputs {
        assert_eq!(
            parse_auth_list(output.as_bytes()),
            ReadinessState::Unknown,
            "contradictory summary became authoritative: {output:?}"
        );
    }
}

#[cfg(unix)]
#[test]
fn authentication_service_errors_remain_unknown() {
    use std::os::unix::fs::PermissionsExt;

    let root = std::env::temp_dir().join(format!(
        "koolade-opencode-auth-503-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let binary = root.join("opencode");
    std::fs::write(
        &binary,
        "#!/bin/sh\necho 'authentication service unavailable (HTTP 503)' >&2\nexit 1\n",
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();

    assert_eq!(probe(&binary), ReadinessState::Unknown);
    std::fs::remove_dir_all(root).unwrap();
}
