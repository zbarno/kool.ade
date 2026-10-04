use super::*;

fn supported_help() -> &'static str {
    "--print\n--mode <mode> text, json, rpc\n--no-session\n--no-approve\n--append-system-prompt\n--thinking <level> xhigh\n--no-extensions\n--no-skills\n--no-prompt-templates\n--no-context-files\n--no-tools\n--tools\n--no-builtin-tools\n--extension"
}

#[test]
fn supported_flags_cover_every_koolade_execution_mode() {
    for mode in ExecutionMode::ALL {
        assert!(
            missing_capabilities(supported_help(), mode).is_empty(),
            "{mode:?}"
        );
    }
}

#[test]
fn unsupported_harness_lists_missing_mode_capabilities() {
    let missing = missing_capabilities(
        "--print\n--mode\n--no-session\n--no-approve\n--thinking low",
        ExecutionMode::Implementation,
    );
    assert!(missing.contains(&"--no-builtin-tools".into()));
    assert!(missing.contains(&"--extension".into()));
    assert!(missing.contains(&"--append-system-prompt".into()));
}

#[test]
fn capability_matching_requires_a_complete_option_name() {
    assert!(has_option("--mode <value>\n", "--mode"));
    assert!(has_option("--print, -p\n", "--print"));
    assert!(!has_option("--modeled <value>\n", "--mode"));
}
