//! F-18 dialog-field pins (pure; no egui): the load -> field mapping
//! decides hint/warning, so unseen content is never destroyed silently.

use crate::app::dialogs::DlgMcp;
use crate::artifacts::mcp_io;

/// First run: no file -> empty field, exemplar hint ACTIVE, no warning.
#[test]
fn mcp_fields_absent_file_opens_empty_with_hint_active() {
    let root = std::env::temp_dir().join(format!("koolade_dlg_mcpabsent_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let st = mcp_io::load_state(&root);
    assert!(!st.present);
    let (text, hint, warning) = DlgMcp::dialog_fields(&st);
    assert_eq!(text, String::new());
    assert!(hint, "first run activates the exemplar hint");
    assert_eq!(warning, None);
    let _ = std::fs::remove_dir_all(&root);
}

/// Present file: content echoed BYTE-EXACT, hint OFF, no warning.
#[test]
fn mcp_fields_present_file_echoes_content_byte_exact() {
    let root = std::env::temp_dir().join(format!("koolade_dlg_mcppresent_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mcp_path = crate::artifacts::repo_artifact(&root, crate::artifacts::MCP_CONFIG_FILE);
    std::fs::create_dir_all(mcp_path.parent().unwrap()).unwrap();
    let body = r#"{"k":1}"#;
    std::fs::write(mcp_path, body).unwrap();
    let st = mcp_io::load_state(&root);
    assert!(st.present);
    let (text, hint, warning) = DlgMcp::dialog_fields(&st);
    assert_eq!(text, body, "editor seeds the exact stored bytes");
    assert!(!hint, "a real file suppresses the exemplar hint");
    assert_eq!(warning, None);
    let _ = std::fs::remove_dir_all(&root);
}

/// Present but UNREADABLE: field starts empty (hint off - there WAS a
/// file) with the sticky warning demanding explicit consent to
/// OVERWRITE unseen content.
#[test]
fn mcp_fields_unreadable_file_warns_explicit_overwrite_consent() {
    let st = mcp_io::McpLoadState {
        present: true,
        content: None,
        read_error: Some("Permission denied (os error 13)".into()),
    };
    let (text, hint, warning) = DlgMcp::dialog_fields(&st);
    assert_eq!(text, String::new(), "field must start empty, not guess");
    assert!(!hint, "unreadable is not absent: hint must not imply empty");
    let warning = warning.expect("sticky warning required for unreadable files");
    assert!(
        warning.contains("could not be read"),
        "warns why: {warning}"
    );
    assert!(
        warning.contains("Permission denied"),
        "quotes the error: {warning}"
    );
    assert!(
        warning.contains("OVERWRITE"),
        "explicit consent wording: {warning}"
    );
}
