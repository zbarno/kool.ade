use super::{ImageLock, RuntimeProfile, task_runtime_profile_order};

#[test]
fn python3_command_is_provided_by_the_canonical_python_capability() {
    let lock = ImageLock::bundled().unwrap();

    let image = lock
        .select_with_tools(&[RuntimeProfile::Python], "amd64", &["python3".into()])
        .unwrap();

    assert_eq!(image.profile, RuntimeProfile::Python);
    assert!(image.provides_tool("python3"));
}

#[test]
fn tool_free_task_targets_choose_the_general_profile_before_language_images() {
    let lock = ImageLock::bundled().unwrap();
    let image = lock
        .select_with_tools(task_runtime_profile_order(&[]), "amd64", &[])
        .unwrap();

    assert_eq!(image.profile, RuntimeProfile::General);
}

#[test]
fn explicit_task_tools_choose_a_matching_language_profile() {
    let lock = ImageLock::bundled().unwrap();
    let required_tools = ["python".into()];
    let image = lock
        .select_with_tools(
            task_runtime_profile_order(&required_tools),
            "amd64",
            &required_tools,
        )
        .unwrap();

    assert_eq!(image.profile, RuntimeProfile::Python);
}

#[test]
fn composition_estimate_sums_constituents_and_treats_gaps_as_unknown() {
    use super::project_plan::CompositionCandidate;

    let mut lock = ImageLock::bundled().unwrap();
    let rust = lock.select(RuntimeProfile::Rust, "amd64").unwrap();
    let node = lock.select(RuntimeProfile::Node, "amd64").unwrap();
    let candidate = CompositionCandidate {
        platform: "linux/amd64".into(),
        foundation: rust.clone(),
        components: vec![node.clone()],
        required_tools: vec!["cargo".into(), "node".into()],
        recipe_sha256: "0".repeat(64),
    };

    let rust_estimate = lock.size_estimate(RuntimeProfile::Rust, "amd64").unwrap();
    let node_estimate = lock.size_estimate(RuntimeProfile::Node, "amd64").unwrap();
    let expected = rust_estimate
        .estimated_storage_bytes
        .saturating_add(node_estimate.estimated_storage_bytes);
    assert_eq!(
        lock.composition_size_estimate(&candidate)
            .map(|value| value.estimated_storage_bytes),
        Some(expected),
        "aggregate must equal the exact constituent sum when every part is known"
    );

    // An estimated gap turns the aggregate into "unknown", never zero:
    // publishing a fabricated number would corrupt progress budgets.
    for variants in lock.profiles.values_mut() {
        for variant in variants.iter_mut() {
            if variant.digest == rust.digest {
                variant.estimated_storage_bytes = None;
            }
        }
    }
    assert!(lock.composition_size_estimate(&candidate).is_none());
}
