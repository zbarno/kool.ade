use super::*;

#[test]
fn opportunity_repository_identity_is_stable_and_workspace_scoped() {
    let workspace = uuid::Uuid::new_v4().hyphenated().to_string();
    let repository = stable_uid(&workspace, "repository", "product");
    assert_eq!(repository, stable_uid(&workspace, "repository", "product"));
    assert_ne!(repository, stable_uid(&workspace, "repository", "root"));
    assert_ne!(
        repository,
        stable_uid(&uuid::Uuid::new_v4().to_string(), "repository", "product")
    );
}
#[test]
fn manifest_rejects_ambiguous_or_machine_specific_entries() {
    let bad = ProjectManifest {
        repositories: vec![
            Repository {
                id: "api".into(),
                role: "Backend".into(),
                remote: "git@example.test:team/api.git".into(),
                display_name: None,
            },
            Repository {
                id: "api".into(),
                role: "Duplicate".into(),
                remote: "git@example.test:team/other.git".into(),
                display_name: None,
            },
        ],
    };
    assert!(bad.validate().is_err());
    let path = Repository {
        id: "../mobile".into(),
        role: "Mobile".into(),
        remote: "/home/user/mobile".into(),
        display_name: None,
    };
    assert!(!valid_id(&path.id));
}

#[test]
fn manifest_rejects_git_remote_helpers_and_option_injection() {
    for remote in [
        "ext::echo injected",
        "custom-transport::payload",
        "custom-transport://payload",
        "--upload-pack=echo injected",
        " --upload-pack=echo injected",
        " /tmp/outside.git",
        "https://example.test/repo.git ",
        "../outside.git",
        "repo.git",
        "owner/repo.git",
    ] {
        let manifest = ProjectManifest {
            repositories: vec![Repository {
                id: "api".into(),
                role: "Backend".into(),
                remote: remote.into(),
                display_name: None,
            }],
        };
        assert!(manifest.validate().is_err(), "accepted {remote:?}");
    }
    assert!(portable_remote("git@example.test:team/repo.git"));
    assert!(portable_remote("ssh://git@example.test/team/repo.git"));
}

#[test]
fn unmanaged_planning_roots_do_not_resolve_arbitrary_local_checkouts() {
    let root = std::env::temp_dir().join(format!(
        "koolade-repos-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let planning = root.join("planning-root");
    let code = root.join("api-checkout");
    std::fs::create_dir_all(&planning).unwrap();
    std::fs::create_dir_all(&code).unwrap();
    for repository in [&planning, &code] {
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q"])
                .current_dir(repository)
                .status()
                .unwrap()
                .success()
        );
    }
    let manifest = ProjectManifest {
        repositories: vec![Repository {
            id: "api".into(),
            role: "Backend API".into(),
            remote: "git@example.test:team/api.git".into(),
            display_name: None,
        }],
    };
    std::fs::create_dir_all(planning.join("config")).unwrap();
    std::fs::write(
        planning.join(PROJECT_FILE),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let loaded = ProjectManifest::load(&planning).unwrap();
    assert_eq!(
        loaded
            .target_if_available(uuid::Uuid::new_v4(), &planning, "api")
            .unwrap(),
        None
    );
    assert!(
        !std::fs::read_to_string(planning.join(PROJECT_FILE))
            .unwrap()
            .contains(code.to_str().unwrap())
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn optional_display_names_are_backward_compatible_and_omit_unset_values() {
    let manifest: ProjectManifest = serde_json::from_str(
        r#"{"repositories":[{"id":"root","role":"Planning root","remote":""}]}"#,
    )
    .unwrap();
    assert_eq!(manifest.repositories[0].display_name, None);
    manifest.validate().unwrap();
    let encoded = serde_json::to_string(&manifest).unwrap();
    assert!(!encoded.contains("display_name"));
}

#[test]
fn display_names_trim_and_duplicate_names_are_allowed() {
    let mut manifest = ProjectManifest {
        repositories: vec![
            Repository {
                id: "api".into(),
                role: "Backend".into(),
                remote: "git@example.test:team/api.git".into(),
                display_name: Some("  Shared label  ".into()),
            },
            Repository {
                id: "web".into(),
                role: "Frontend".into(),
                remote: "git@example.test:team/web.git".into(),
                display_name: Some("Shared label".into()),
            },
        ],
    };
    manifest.normalize_display_names().unwrap();
    manifest.validate().unwrap();
    assert_eq!(
        manifest.repositories[0].display_name.as_deref(),
        Some("Shared label")
    );
    let encoded = serde_json::to_string(&manifest).unwrap();
    assert!(encoded.contains("Shared label"));
    assert!(!encoded.contains("  Shared label  "));
}

#[test]
fn manifest_rejects_display_names_over_limit_or_with_controls() {
    let repository = |display_name| Repository {
        id: "root".into(),
        role: "Planning root".into(),
        remote: String::new(),
        display_name,
    };
    let too_long = ProjectManifest {
        repositories: vec![repository(Some("x".repeat(41)))],
    };
    assert!(too_long.validate().unwrap_err().to_string().contains("40"));
    let control = ProjectManifest {
        repositories: vec![repository(Some("bad\u{1b}name".into()))],
    };
    assert!(
        control
            .validate()
            .unwrap_err()
            .to_string()
            .contains("control")
    );
}
