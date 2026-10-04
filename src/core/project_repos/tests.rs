use super::*;
#[test]
fn manifest_rejects_ambiguous_or_machine_specific_entries() {
    let bad = ProjectManifest {
        repositories: vec![
            Repository {
                id: "api".into(),
                role: "Backend".into(),
                remote: "git@example/api".into(),
                display_name: None,
            },
            Repository {
                id: "api".into(),
                role: "Duplicate".into(),
                remote: "git@example/other".into(),
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
fn portable_manifest_resolves_private_checkout_without_committing_paths() {
    let root = std::env::temp_dir().join(format!(
        "koolade-repos-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let planning = root.join("planning-root");
    let api = root.join("api-checkout");
    std::fs::create_dir_all(planning.join(crate::artifacts::layout::canonical::CONFIG)).unwrap();
    std::fs::create_dir_all(&api).unwrap();
    for repo in [&planning, &api] {
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q"])
                .current_dir(repo)
                .status()
                .unwrap()
                .success()
        );
    }
    assert!(
        std::process::Command::new("git")
            .args(["remote", "add", "origin", "git@example.test:team/api.git"])
            .current_dir(&api)
            .status()
            .unwrap()
            .success()
    );
    let rewrite = format!("url.{}.insteadOf", root.join("mirror.git").display());
    assert!(
        std::process::Command::new("git")
            .args(["config", "--local", &rewrite, "git@example.test:"])
            .current_dir(&api)
            .status()
            .unwrap()
            .success()
    );
    let effective = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(&api)
        .output()
        .unwrap();
    assert!(effective.status.success());
    assert_ne!(
        String::from_utf8(effective.stdout).unwrap().trim(),
        "git@example.test:team/api.git"
    );
    let manifest = ProjectManifest {
        repositories: vec![
            Repository {
                id: "planning".into(),
                role: "Planning root".into(),
                remote: "git@example.test:team/planning.git".into(),
                display_name: None,
            },
            Repository {
                id: "api".into(),
                role: "Backend API".into(),
                remote: "git@example.test:team/api.git".into(),
                display_name: None,
            },
        ],
    };
    std::fs::write(
        planning.join(PROJECT_FILE),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    map_local_checkout(&planning, "api", &api).unwrap();
    assert_eq!(
        ProjectManifest::load(&planning)
            .unwrap()
            .target(&planning, "api")
            .unwrap(),
        api.canonicalize().unwrap()
    );
    assert!(
        !std::fs::read_to_string(planning.join(PROJECT_FILE))
            .unwrap()
            .contains(api.to_str().unwrap())
    );
    let private = crate::persistence::project_dir(&crate::persistence::project_slug(
        &planning.canonicalize().unwrap(),
    ));
    let _ = std::fs::remove_dir_all(private);
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
                remote: "git@example/api".into(),
                display_name: Some("  Shared label  ".into()),
            },
            Repository {
                id: "web".into(),
                role: "Frontend".into(),
                remote: "git@example/web".into(),
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
