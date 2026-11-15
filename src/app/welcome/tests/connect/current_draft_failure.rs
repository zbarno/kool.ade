use super::*;

/// Production boundary: reconnect must fail closed on a malformed current
/// spec draft and retain its exact bytes for diagnosis or recovery. Store
/// tests cannot prove the welcome path surfaces this failure without silently
/// loading partial project state. Uses one temporary local Git repo/private
/// store; no provider, Docker, or network; low setup cost.
#[test]
fn malformed_current_draft_fails_reconnect_without_rewriting_evidence() {
    let _env = super::super::helpers::EnvSandbox::enter("malformed-current-draft", false);
    let code = super::super::helpers::local_source_repo("malformed-current-draft");
    let project = attempt_connect(code.to_str().unwrap()).unwrap();
    let draft_path = project
        .state
        .planning_store
        .root
        .join("state/spec_drafts/current.json");
    let malformed = br#"{"schemaVersion":99,"basePublishedSpecPin":null,"changes":[],"proposalSha256":"0000000000000000000000000000000000000000000000000000000000000000"}"#;
    project
        .state
        .planning_store
        .atomic_write("state/spec_drafts/current.json", malformed)
        .unwrap();
    drop(project);

    let error = match attempt_connect(code.to_str().unwrap()) {
        Ok(_) => panic!("malformed draft must block reconnect"),
        Err(error) => error,
    };
    assert!(
        error
            .to_string()
            .contains("unsupported local specification proposal schema"),
        "reconnect should report the draft validation failure, got: {error}"
    );
    assert_eq!(std::fs::read(&draft_path).unwrap(), malformed);
    let _ = std::fs::remove_dir_all(code);
    let _ = std::fs::remove_dir_all(&_env.home);
}
