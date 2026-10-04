use super::*;

#[test]
fn report_migration_and_blocker_classification_use_typed_cause() {
    let old = r#"{"status":"blocked","summary":"The old report lacks a disposition.","acceptance_criteria":[],"verification":[],"remaining":["Adjudicator: review"]}"#;
    let migrated = parse_report(old).unwrap();
    assert!(external_blocker(&migrated));

    let current = r#"{"schemaVersion":1,"status":"blocked","blocker_disposition":"machine_repair","summary":"Display test needs a code fix.","acceptance_criteria":[],"verification":[],"remaining":["Operator: run the display workstation test"]}"#;
    assert!(!external_blocker(&parse_report(current).unwrap()));

    let invalid_current = r#"{"schemaVersion":1,"status":"blocked","summary":"Missing required type.","acceptance_criteria":[],"verification":[],"remaining":[]}"#;
    assert!(parse_report(invalid_current).is_err());
}

#[test]
fn report_v2_carries_dynamic_issue_choices_and_v1_defaults_empty() {
    let current = r#"{"schemaVersion":2,"status":"blocked","blocker_disposition":"human_action","summary":"Published history conflicts with this task's footprint.","acceptance_criteria":[],"verification":[],"remaining":["Adjudicator: select the policy."],"human_choices":[{"id":"effective-base","label":"Use the published base","meaning":"Compare against the actual branch starting point.","consequence":"The counted files include earlier published work."},{"id":"rewrite-rule","label":"Correct the footprint rule","meaning":"Measure only files added for this task.","consequence":"The written acceptance rule changes."}]}"#;
    let parsed = parse_report(current).unwrap();
    assert_eq!(parsed.human_choices.len(), 2);
    assert!(validate_human_choices(&parsed).is_ok());
    let detail = external_blocker_detail(&parsed, Path::new("report.json"));
    assert!(detail.contains("Use the published base"));
    assert!(detail.contains("The counted files include earlier published work."));

    let legacy = parse_report(r#"{"schemaVersion":1,"status":"blocked","blocker_disposition":"human_action","summary":"Needs a person.","acceptance_criteria":[],"verification":[],"remaining":["Owner: wait for reset"]}"#).unwrap();
    assert!(legacy.human_choices.is_empty());
}

#[test]
fn missing_sandbox_dependencies_are_environment_blockers() {
    let text = r#"{"schemaVersion":2,"status":"blocked","blocker_disposition":"environment_prerequisite","summary":"The sandbox has no npm cache and networking is disabled.","acceptance_criteria":[],"verification":["npm ci"],"remaining":["Seed the npm cache, then rerun frontend checks."],"human_choices":[]}"#;
    let report = parse_report(text).unwrap();
    assert!(external_blocker(&report));
    let detail = external_blocker_detail(&report, Path::new("report.json"));
    assert!(detail.starts_with("## Waiting for environment"));
    assert!(detail.contains("Seed the npm cache"));
    assert!(detail.contains("resume implementation"));
}

#[test]
fn blocker_guidance_does_not_seed_issue_specific_remedies() {
    let instructions = feasibility_preflight().to_ascii_lowercase();
    for unrelated_remedy in [
        "realized footprint",
        "revise the predicate",
        "history repair",
        "quota",
    ] {
        assert!(
            !instructions.contains(unrelated_remedy),
            "shared blocker guidance must not seed {unrelated_remedy}"
        );
    }
    assert!(instructions.contains("derive any decision options only from this task's evidence"));
    assert!(instructions.contains("report a step instead of inventing choices"));
}

#[test]
fn legacy_null_fields_keep_issue_choices_for_the_generated_explanation() {
    let legacy = r#"{"status":"blocked","blocker_disposition":null,"summary":"The published file list conflicts with this task's frozen requirement.","acceptance_criteria":[],"verification":[],"remaining":["Adjudicator: pick exactly one remedy: (a) ratify the effective base; (b) reissue the footprint rule; (c) approve exemptions; or (d) authorize history repair."],"human_choices":null}"#;
    let report = parse_report(legacy).unwrap();
    assert_eq!(report.blocker_disposition, BlockerDisposition::HumanAction);
    assert!(report.human_choices.is_empty());
    assert_eq!(report.remaining.len(), 1);
    assert!(report.remaining[0].contains("ratify the effective base"));
}
