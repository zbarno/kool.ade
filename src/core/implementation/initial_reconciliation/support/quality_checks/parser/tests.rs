use super::*;

#[test]
fn captures_inline_custom_and_fenced_required_commands() {
    let commands = required_commands_in_markdown(
        "## Quality Gates\n\n- Run `./scripts/quality.sh --strict` and `nix develop -c cargo test`.\n\nBefore marking complete, run these too:\n```sh\nbash scripts/smoke.sh\n```\n",
        Path::new(""),
    );

    assert_eq!(
        commands.unwrap(),
        [
            "./scripts/quality.sh --strict",
            "nix develop -c cargo test",
            "bash scripts/smoke.sh"
        ]
    );
}

#[test]
fn does_not_turn_plain_quality_guidance_into_a_command() {
    let commands = required_commands_in_markdown(
        "## Quality Gates\n\n- Run every required project test before completion.\n",
        Path::new(""),
    );

    assert!(commands.unwrap().is_empty());
}

#[test]
fn validation_entry_point_catalog_is_not_a_required_gate() {
    let commands = required_commands_in_markdown(
        "## Validation entry points\n\n- Backend build: `dotnet build`\n- Backend run: `dotnet run` from `xleratehealth/`\n- Backend tests: `dotnet test`\n- Frontend tests: `npm run test --prefix xleratehealth/ClientApp -- --runTestsByPath <path>`\n",
        Path::new("Source"),
    )
    .unwrap();

    assert!(commands.is_empty());
}

#[test]
fn conditional_validation_gate_is_selected_only_for_matching_changes() {
    let instructions = "## Validation entry points\n\n- Backend build: `dotnet build`\n- Backend run: `dotnet run` from `xleratehealth/`\n- Frontend lint: `npm run lint --prefix xleratehealth/ClientApp` — required after any change under `ClientApp/src`, and must stay at 0 errors\n";
    let backend_changes = [PathBuf::from(
        "Source/xleratehealth/Controllers/StartupController.cs",
    )];
    let frontend_changes = [PathBuf::from(
        "Source/xleratehealth/ClientApp/src/components/StartupProfile.tsx",
    )];

    let backend_commands = required_commands_in_markdown_for_changes(
        instructions,
        Path::new("Source"),
        &backend_changes,
    )
    .unwrap();
    let frontend_commands = required_commands_in_markdown_for_changes(
        instructions,
        Path::new("Source"),
        &frontend_changes,
    )
    .unwrap();

    assert!(backend_commands.is_empty());
    assert_eq!(
        frontend_commands,
        ["cd -- 'Source' && npm run lint --prefix xleratehealth/ClientApp"]
    );
}

#[test]
fn conditional_validation_gate_does_not_match_a_same_named_nested_package() {
    let instructions = "## Validation entry points\n\n- Frontend lint: `npm run lint --prefix xleratehealth/ClientApp` — required after any change under `ClientApp/src`, and must stay at 0 errors\n";
    let unrelated_changes = [PathBuf::from(
        "Source/AnotherPackage/ClientApp/src/StartupProfile.tsx",
    )];

    let commands = required_commands_in_markdown_for_changes(
        instructions,
        Path::new("Source"),
        &unrelated_changes,
    )
    .unwrap();

    assert!(commands.is_empty());
}

#[test]
fn explicitly_required_validation_entry_point_heading_is_a_gate() {
    let commands = required_commands_in_markdown(
        "## Required validation entry points\n\n- Backend tests: `dotnet test`\n",
        Path::new(""),
    )
    .unwrap();

    assert_eq!(commands, ["dotnet test"]);
}

#[test]
fn explicit_validation_directory_cannot_escape_the_worktree() {
    assert!(
        required_commands_in_markdown(
            "## Validation\n\n- Run `dotnet test` from `../../outside/`\n",
            Path::new("Source"),
        )
        .is_err()
    );
}

#[test]
fn nested_cd_commands_are_combined_with_the_instruction_directory() {
    let commands = required_commands_in_markdown(
        "## Validation\n\n- Run `cd scripts && npm test`\n",
        Path::new("Source"),
    )
    .unwrap();

    assert_eq!(commands, ["cd -- 'Source/scripts' && npm test"]);
}

#[test]
fn nested_cd_commands_cannot_escape_the_instruction_directory() {
    assert!(
        required_commands_in_markdown(
            "## Validation\n\n- Run `cd .. && npm test`\n",
            Path::new("Source"),
        )
        .is_err()
    );
}

#[test]
fn report_checks_in_different_directories_are_not_treated_as_covered() {
    let required = vec!["cd -- 'Source' && dotnet test".to_owned()];

    assert!(!report_check_is_covered(
        &required,
        "cd -- 'ClientApp' && dotnet test"
    ));
    assert!(report_check_is_covered(&required, "dotnet test"));
}

#[test]
fn a_bare_report_check_covers_only_one_matching_required_body() {
    let required = vec![
        "cd -- 'Source' && dotnet test".to_owned(),
        "cd -- 'ClientApp' && npm test".to_owned(),
    ];
    let ambiguous = vec![
        "cd -- 'Source' && dotnet test".to_owned(),
        "cd -- 'ClientApp' && dotnet test".to_owned(),
    ];

    assert!(report_check_is_covered(&required, "dotnet test"));
    assert!(!report_check_is_covered(&ambiguous, "dotnet test"));
}
