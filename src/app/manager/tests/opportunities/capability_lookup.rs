use super::{git, initialized_repository};

/// Production boundary: the workspace Manager prompt builder binds a private
/// user goal to a bounded catalog lookup without marking it public. A unit
/// lookup test cannot prove prompt evidence classification or builder wiring.
/// Requires only temporary Git and filesystem access; setup cost is low.
#[test]
fn manager_prompt_uses_bounded_goal_specific_product_capability_lookup() {
    let (_temp, mut project, _) = initialized_repository();
    project.chat.push(crate::domain::chatlog::ChatMessage::new(
        crate::domain::chatlog::ChatRole::User,
        "How do I run repository runtime tests?",
        None,
    ));
    let prompt = super::super::super::catalog::render(&project, &[]).unwrap();
    let evidence = prompt
        .validation
        .evidence_content
        .get("product-capability-lookup")
        .expect("goal-scoped lookup is included in the Manager prompt");
    assert!(evidence.contains("repository-runtime-qa"));
    assert!(evidence.contains("ImplementedInBuild") || evidence.contains("Unknown"));
    assert!(evidence.len() <= 8_000);
    assert!(
        !prompt
            .validation
            .public_evidence_ids
            .contains("product-capability-lookup")
    );

    project.chat.last_mut().unwrap().text = "quantum teleportation simulator".into();
    let prompt = super::super::super::catalog::render(&project, &[]).unwrap();
    let evidence = prompt
        .validation
        .evidence_content
        .get("product-capability-lookup")
        .unwrap();
    assert!(evidence.contains("Capability lookup is unknown"));
    assert!(evidence.contains("not evidence that the capability is absent"));
}

/// Production boundary: the workspace Manager receives local architecture
/// evidence with a source path and commit, while the product-only evidence set
/// stays separate. A scanner unit test cannot prove evidence reaches the
/// Manager prompt with workspace-private classification. Uses temporary Git
/// and filesystem only; no Docker/network; setup cost is low.
#[test]
fn manager_prompt_receives_revision_pinned_repository_profile() {
    let (temp, mut project, _) = initialized_repository();
    project.chat.push(crate::domain::chatlog::ChatMessage::new(
        crate::domain::chatlog::ChatRole::User,
        "Explain the repository architecture and entry point.",
        None,
    ));
    let root = temp.path().join("code");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    git(&root, &["add", "src/main.rs"]);
    git(&root, &["commit", "-qm", "add sample entry point"]);
    project.state =
        crate::core::state::PlannerState::load_with_store(&root, &project.state.planning_store)
            .unwrap();

    let prompt = super::super::super::catalog::render(&project, &[]).unwrap();
    let profile = prompt
        .validation
        .evidence_content
        .get("repository-architecture-profile")
        .expect("workspace Manager includes its repository profile");
    assert!(profile.contains("Repository root at checked-out ref refs/heads/main commit"));
    assert!(profile.contains("src/main.rs:1-1@"));
    assert!(profile.contains("entry points and interfaces"));
    assert!(profile.len() <= 8_000);
    assert!(
        !prompt
            .validation
            .public_evidence_ids
            .contains("repository-architecture-profile")
    );
}
