use super::*;

#[test]
fn auth_profile_contains_only_the_selected_model_provider() {
    let auth = json!({
        "anthropic": { "type": "api", "key": "selected-synthetic-token" },
        "openai": { "type": "api", "key": "unselected-synthetic-token" },
    });
    let profile = selected_auth(&auth, "anthropic").unwrap();
    assert_eq!(profile["anthropic"]["key"], "selected-synthetic-token");
    assert!(profile.get("openai").is_none());
    assert_eq!(profile.as_object().unwrap().len(), 1);
    assert!(selected_auth(&auth, "missing-provider").is_none());
}

#[test]
fn jsonc_parser_preserves_comment_markers_in_strings_and_strips_trailing_commas() {
    let config = parse_config(
        r#"{ // comment
        "model": "provider/model//v1",
        "provider": { "custom": { "npm": "unsafe", "options": {}, }, },
    }"#,
    )
    .unwrap();
    assert_eq!(config["model"], "provider/model//v1");
    let mut output = json!({ "provider": {} });
    merge_provider_values(&mut output, config);
    assert!(output["provider"]["custom"].get("npm").is_none());
}
