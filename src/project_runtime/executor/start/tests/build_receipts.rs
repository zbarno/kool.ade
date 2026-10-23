use super::*;

#[test]
fn start_rejects_an_older_passed_build_after_a_newer_incomplete_attempt() {
    let mut manifest = super::manifest();
    let target = &mut manifest.candidate_targets[0];
    target.build_recipe_ref = Some("build-web".into());
    target.artifact_spec_ref = Some("web-dist".into());
    manifest
        .artifact_specs
        .insert("web-dist".into(), vec!["dist/index.html".into()]);
    let passed = super::build_receipt("apps/dist/index.html");

    for (state, exit_code) in [
        (RuntimeReceiptState::Running, None),
        (RuntimeReceiptState::Failed, Some(1)),
    ] {
        let mut latest = passed.clone();
        latest.receipt_id = uuid::Uuid::new_v4().hyphenated().to_string();
        latest.state = state;
        latest.exit_code = exit_code;
        latest.observed_at_unix_ms = 2;

        assert!(
            super::build_artifacts(
                &manifest,
                &manifest.candidate_targets[0],
                &[passed.clone(), latest],
                &"a".repeat(40),
            )
            .is_err(),
            "latest {state:?} Build receipt must supersede the older pass"
        );
    }
}
