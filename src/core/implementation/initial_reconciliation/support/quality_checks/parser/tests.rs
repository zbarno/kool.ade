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
fn commands_use_the_instruction_directory_and_explicit_project_directory() {
    let commands = required_commands_in_markdown(
        "## Validation entry points\n\n- Backend build: `dotnet build`\n- Backend run: `dotnet run` from `xleratehealth/`\n- Backend tests: `dotnet test`; narrow with `--filter \\\"FullyQualifiedName~Controller\\\"` when useful.\n",
        Path::new("Source"),
    )
    .unwrap();

    assert_eq!(
        commands,
        [
            "cd -- 'Source' && dotnet build",
            "cd -- 'Source/xleratehealth' && dotnet run",
            "cd -- 'Source' && dotnet test",
        ]
    );
    assert!(report_check_is_covered(&commands, "dotnet build"));
    assert!(report_check_is_covered(
        &commands,
        "cd -- 'Source' && dotnet build"
    ));
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
