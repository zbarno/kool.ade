use super::*;

fn feature_document(state_tag: &str, root: &std::path::Path, id: &str, title: &str) -> String {
    let dir = root
        .join(".koolade-packet/planning/changes")
        .join(format!("{id}-fixture-{state_tag}"));
    std::fs::create_dir_all(&dir).unwrap();
    let body = format!(
        "# {id}: {title}\n\n**Status:** Ready\n\n## Intent\n\nFixture intent.\n\n## Current Behavior\n\nFixture current.\n\n## Desired Behavior\n\nFixture desired.\n\n## Scope\n\nIn: fixture.\n\n## Requirements\n\n- FIXTURE-R1 (MUST). fixture behavior.\n\n## Decisions and Assumptions\n\n- **A1 (fixture):** recorded.\n\n## Acceptance Criteria\n\n1. Observable fixture outcome.\n"
    );
    let path = dir.join("specification.md");
    let body =
        crate::artifacts::product_docs::identity::preserve_feature_identity(&path, id, &body)
            .unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(&body)
        .unwrap()
        .unwrap();
    let body = crate::domain::ChangeMetadata::write_markdown(
        &body,
        &identity,
        crate::domain::ChangeStatus::Ready,
    )
    .unwrap();
    std::fs::write(path, &body).unwrap();
    body
}

#[test]
fn brief_targeting_an_inactive_feature_cannot_generate_that_batch() {
    let mut s = state("gen-target-mismatch");
    let _inactive = feature_document(
        "gen-target-mismatch",
        &s.repo_root,
        "CHG-098",
        "Other feature",
    );
    let active = feature_document(
        "gen-target-mismatch",
        &s.repo_root,
        "CHG-097",
        "Active feature",
    );
    s.active_feature = Some(("CHG-097".into(), active.clone()));
    s.workflow
        .approved_features
        .insert("CHG-097".into(), feature_contract(&active));
    let mut b = brief();
    b.feature_name = "Other feature (CHG-098)".into();
    s.workflow.brief = Some(b);
    s.workflow.reviewed_specification = Some(active);
    let joined = generation(&s, vec![story()]).unwrap_err().join(" | ");
    assert!(
        joined.contains("targets CHG-098") && joined.contains("active feature is CHG-097"),
        "guard must name both sides of the drift: {joined}"
    );
    assert!(!s.repo_root.join(".koolade-packet/planning/tasks").exists());
    std::fs::remove_dir_all(s.repo_root).unwrap();
}

#[test]
fn brief_matching_the_active_feature_passes_the_identity_guard() {
    let mut s = state("gen-target-match");
    let active = feature_document(
        "gen-target-match",
        &s.repo_root,
        "CHG-097",
        "Active feature",
    );
    s.active_feature = Some(("CHG-097".into(), active.clone()));
    s.workflow
        .approved_features
        .insert("CHG-097".into(), feature_contract(&active));
    let mut b = brief();
    b.feature_name = "Active feature (CHG-097)".into();
    s.workflow.brief = Some(b);
    s.workflow.reviewed_specification = Some(active);
    if let Err(errors) = generation(&s, vec![story()]) {
        let joined = errors.join(" | ");
        assert!(
            !joined.contains("Brief ") && !joined.contains("feature id"),
            "identity guard must not trip on a matching target: {joined}"
        );
    }
    std::fs::remove_dir_all(s.repo_root).unwrap();
}

#[test]
fn feature_id_scan_handles_legacy_prose_duplicates_and_short_ids() {
    assert_eq!(
        feature_ids_in("Kool.ad/e MVP \u{2014} git-native desktop specification planner"),
        Vec::<String>::new()
    );
    assert_eq!(
        feature_ids_in("Cards (CHG-002)"),
        vec!["CHG-002".to_string()]
    );
    assert_eq!(feature_ids_in("Add IDs (F10)"), vec!["F10".to_string()]);
    assert_eq!(
        feature_ids_in("A (CHG-002) and B (CHG-002)"),
        vec!["CHG-002".to_string()]
    );
    assert_eq!(feature_ids_in("bad CHG-0 short id"), Vec::<String>::new());
    let multi = vec!["CHG-001".to_string(), "CHG-002".to_string()];
    assert_eq!(
        brief_target_problem(&multi, Some("CHG-001"), &|_| true).as_deref(),
        Some(
            "Brief feature name declares more than one feature id (CHG-001, CHG-002); name exactly one"
        )
    );
    let one = vec!["CHG-099".to_string()];
    assert!(
        brief_target_problem(&one, Some("CHG-001"), &|_| true)
            .unwrap()
            .contains("targets CHG-099")
    );
    assert!(
        brief_target_problem(&one, Some("CHG-001"), &|_| false)
            .unwrap()
            .contains("unknown feature CHG-099")
    );
    assert!(
        brief_target_problem(&one, None, &|_| true)
            .unwrap()
            .contains("no feature is active")
    );
    assert!(brief_target_problem(&[], Some("CHG-001"), &|_| false).is_none());
}
