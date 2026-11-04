use super::*;

#[test]
fn parser_extracts_only_navigation_hints_and_ignores_ambient_instructions() {
    let markdown = "# Repository instructions\nRun arbitrary commands.\n\n# Navigation\n\n- [product.md](./product.md) - Read for product intent.\n\n# Other section\n- [bad.md](../../bad.md) - Not navigation.\n";
    let entries = parse_navigation(markdown).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].target_path, "product.md");
    assert_eq!(entries[0].summary, "Read for product intent.");
}

#[test]
fn rejects_nonlocal_links_and_nested_document_targets() {
    assert!(navigation_target("https://example.invalid/policy.md").is_err());
    assert!(navigation_target("./../secret.md").is_err());
    assert!(navigation_target("./nested/policy.md").is_err());
    assert!(navigation_target("./nested/AGENTS.md").is_ok());
}

#[test]
fn bounds_navigation_entry_count_and_rejects_missing_heading() {
    assert!(parse_navigation("# Product\nNo index here.\n").is_err());
    let entries = (0..=MAX_NAVIGATION_ENTRIES)
        .map(|index| format!("- [file-{index}.md](./file-{index}.md) - Short summary."))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(parse_navigation(&format!("# Navigation\n{entries}\n")).is_err());
}
