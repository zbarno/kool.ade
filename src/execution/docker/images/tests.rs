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
