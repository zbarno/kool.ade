use super::*;

#[test]
// Production boundary: clarification planning receives a bounded, read-only
// repository overview as citable evidence. Unit tests of the prompt schema cannot
// prove actual README, manifest, and tree contents are collected from the checkout.
// Dependencies: temporary synthetic repository; no services.
// Setup cost: low, with three small filesystem entries.
fn evidence_contains_readme_manifest_and_shallow_tree() {
    let root = std::env::temp_dir().join(format!("planner-evidence-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("README.md"),
        "Repository precedent: use typed adapters.\n",
    )
    .unwrap();
    std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"fixture\"\n").unwrap();
    std::fs::write(root.join("src/lib.rs"), "pub fn synthetic() {}\n").unwrap();
    let state = crate::core::state::PlannerState::load(&root).unwrap();

    let evidence = for_feature(&state, "feature-uid", "workspace-uid");

    assert_eq!(evidence.id, "repository-overview:feature-uid");
    assert!(
        evidence
            .content
            .contains("Repository precedent: use typed adapters.")
    );
    assert!(evidence.content.contains("cargo.toml"));
    assert!(evidence.content.contains("src/lib.rs"));
    assert!(evidence.content.len() <= 12 * 1024);
    let _ = std::fs::remove_dir_all(root);
}
