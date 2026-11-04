use super::{WelcomeEntry, WelcomeWizard};

#[test]
fn dotted_github_repository_keeps_its_full_code_name() {
    let mut wizard = WelcomeWizard::new(WelcomeEntry::CloneRepository);
    wizard.url = "https://github.com/example/kool.ade".into();

    assert_eq!(wizard.code_folder_name(), "kool.ade");
    assert!(wizard.disk_plan_lines()[0].contains("example/kool.ade"));
    assert_eq!(wizard.disk_plan_lines().len(), 1);
    assert!(wizard.clone_destination().unwrap().ends_with("kool.ade"));
}

#[test]
fn dotted_github_repository_with_git_suffix_keeps_its_full_name() {
    let mut wizard = WelcomeWizard::new(WelcomeEntry::CloneRepository);
    wizard.url = "https://github.com/example/kool.ade.git".into();

    assert_eq!(wizard.code_folder_name(), "kool.ade");
    assert_eq!(wizard.code_folder_name(), "kool.ade");
}
