use super::{WelcomeEntry, WelcomeWizard};

#[test]
fn dotted_github_repository_keeps_its_full_code_and_planning_names() {
    let mut wizard = WelcomeWizard::new(WelcomeEntry::CloneRepository);
    wizard.url = "https://github.com/example/kool.ade".into();

    assert_eq!(wizard.code_folder_name(), "kool.ade");
    assert_eq!(wizard.effective_project_name(), "kool.ade");
    assert_eq!(
        wizard.effective_planning_repository_name(),
        "kool.ade.packet"
    );
    assert!(
        wizard
            .disk_plan_lines()
            .iter()
            .any(|line| line.contains("own branch and Git history"))
    );
    assert!(wizard.clone_destination().unwrap().ends_with("kool.ade"));
}

#[test]
fn dotted_github_repository_with_git_suffix_keeps_its_full_name() {
    let mut wizard = WelcomeWizard::new(WelcomeEntry::CloneRepository);
    wizard.url = "https://github.com/example/kool.ade.git".into();

    assert_eq!(wizard.code_folder_name(), "kool.ade");
    assert_eq!(
        wizard.effective_planning_repository_name(),
        "kool.ade.packet"
    );
}
