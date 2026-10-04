use super::references_module;

#[test]
fn stale_plan_choice_invalidates_a_feature_batch_contract() {
    let root = std::env::temp_dir().join(format!("koolade-batch-contract-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let layout = crate::artifacts::layout::ArtifactLayout::new(&root);
    let batch = layout.task_batch("F7-plan-a").unwrap();
    std::fs::create_dir_all(&batch).unwrap();
    let plan_a = "## Selected Plan\n\nPlan A\n";
    let plan_b = "## Selected Plan\n\nPlan B\n";
    let snapshot = super::BatchContract {
        feature_id: "F7".into(),
        feature_specification: plan_a.into(),
        product_modules: Default::default(),
        repository_bases: Default::default(),
        configuration: String::new(),
    };
    std::fs::write(
        batch.join("contract.json"),
        serde_json::to_vec(&snapshot).unwrap(),
    )
    .unwrap();
    let directory = format!("{}/F7-plan-a", crate::artifacts::layout::canonical::TASKS);
    assert!(super::batch_contract_matches_feature(
        &root, &directory, "F7", plan_a
    ));
    assert!(!super::batch_contract_matches_feature(
        &root, &directory, "F7", plan_b
    ));
    assert!(!super::batch_contract_matches_feature(
        &root,
        "../../outside",
        "F7",
        plan_b
    ));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn numeric_product_module_references_match_manifest_ids() {
    assert!(references_module(
        "Affected Product Areas: Module 05 (functional requirements)",
        "05-functional-requirements",
        "05-functional-requirements.md",
        "5. Functional Requirements",
    ));
    assert!(references_module(
        "Change affects module 5.",
        "05-functional-requirements",
        "05-functional-requirements.md",
        "5. Functional Requirements",
    ));
}

#[test]
fn numeric_module_references_keep_boundaries_and_stable_ids() {
    assert!(!references_module(
        "Affected Product Areas: Module 050",
        "05-functional-requirements",
        "05-functional-requirements.md",
        "5. Functional Requirements",
    ));
    assert!(references_module(
        "Affected Product Areas: product:05-functional-requirements",
        "05-functional-requirements",
        "05-functional-requirements.md",
        "5. Functional Requirements",
    ));
}

#[test]
fn legacy_batch_without_snapshot_remains_current_for_compatibility() {
    let root = std::env::temp_dir().join(format!("koolade-legacy-batch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let batch = crate::artifacts::layout::ArtifactLayout::new(&root)
        .task_batch("legacy")
        .unwrap();
    std::fs::create_dir_all(&batch).unwrap();
    let directory = format!("{}/legacy", crate::artifacts::layout::canonical::TASKS);
    assert!(super::batch_contract_matches_feature(
        &root,
        &directory,
        "CHG-001",
        "Current feature specification",
    ));
    let _ = std::fs::remove_dir_all(root);
}
