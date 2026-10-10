use std::fs;

use super::support::Fixture;
use crate::core::managed_workspace::migration::MigrationFailurePoint;

#[test]
fn interrupted_migrations_resume_with_the_journaled_workspace_id() {
    for (label, point) in [
        ("after-journal", MigrationFailurePoint::JournalWritten),
        ("after-commit", MigrationFailurePoint::InitialCommitCreated),
        ("after-rename", MigrationFailurePoint::RepositoryRenamed),
        (
            "after-association",
            MigrationFailurePoint::AssociationWritten,
        ),
    ] {
        let fixture = Fixture::new(label);
        let error = fixture
            .manager
            .migrate_legacy_checkout_with_failure(&fixture.code, point)
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("injected planning migration interruption")
        );
        let id = journal_id(&fixture.root.join("operator/projects/.migrations"));

        let resumed = fixture
            .manager
            .migrate_legacy_checkout(&fixture.code)
            .unwrap();

        assert_eq!(resumed.id, id, "workspace UUID changed after {label}");
        assert_eq!(resumed.manifest.workspace_id, id);
        assert!(fixture.manager.association_path(id).is_file());
        assert!(
            fs::read_to_string(resumed.planning_repo.join("migration-report.json"))
                .unwrap()
                .contains(&id.to_string())
        );
    }
}

#[test]
fn generated_workspace_uuid_is_journaled_before_staging_and_reused() {
    let fixture = Fixture::new_without_legacy_workspace_id("generated-identity");
    let error = fixture
        .manager
        .migrate_legacy_checkout_with_failure(&fixture.code, MigrationFailurePoint::JournalWritten)
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("injected planning migration interruption")
    );
    let id = journal_id(&fixture.root.join("operator/projects/.migrations"));
    assert!(!id.is_nil());

    let workspace = fixture
        .manager
        .migrate_legacy_checkout(&fixture.code)
        .unwrap();

    assert_eq!(workspace.id, id);
    assert!(fixture.legacy_workspace_id.is_none());
}

#[test]
fn recovery_refuses_to_associate_a_modified_renamed_repository() {
    let fixture = Fixture::new("recovery-modified-destination");
    let error = fixture
        .manager
        .migrate_legacy_checkout_with_failure(
            &fixture.code,
            MigrationFailurePoint::RepositoryRenamed,
        )
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("injected planning migration interruption")
    );
    let id = journal_id(&fixture.root.join("operator/projects/.migrations"));
    let planning_repo = fixture.manager.project_dir(id).join("planning-repo");
    fs::write(
        planning_repo.join("config/project.md"),
        b"tampered after rename\n",
    )
    .unwrap();

    let error = fixture
        .manager
        .migrate_legacy_checkout(&fixture.code)
        .unwrap_err();

    assert!(error.to_string().contains("uncommitted changes"));
    assert!(!fixture.manager.association_path(id).exists());
}

fn journal_id(directory: &std::path::Path) -> uuid::Uuid {
    let entry = fs::read_dir(directory).unwrap().next().unwrap().unwrap();
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(entry.path()).unwrap()).unwrap();
    uuid::Uuid::parse_str(value["workspaceId"].as_str().unwrap()).unwrap()
}
