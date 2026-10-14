use super::strip_target_root;

#[test]
fn target_root_prefix_requires_a_path_boundary() {
    assert_eq!(strip_target_root("apps/web", "apps/web").unwrap(), ".");
    assert_eq!(
        strip_target_root("apps/web", "apps/web/src").unwrap(),
        "src"
    );
    assert!(strip_target_root("apps/web", "apps/webish/src").is_err());
    assert!(strip_target_root("apps/web", "apps/api/src").is_err());
}
