use super::initialized_repository;

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
