use super::*;

fn normalized_image_id(raw: &str) -> String {
    if raw.starts_with("sha256:") {
        raw.to_owned()
    } else {
        format!("sha256:{raw}")
    }
}

/// Remove only the exact tag still pointing to the test-owned image. Never
/// force removal or delete an image by ID, which could have gained other tags.
struct OwnedTag {
    tag: String,
    image_id: String,
}

impl OwnedTag {
    fn remove_checked(&self) {
        let actual = docker_run(&["image", "inspect", "--format", "{{.Id}}", &self.tag]);
        assert_eq!(
            actual, self.image_id,
            "test-owned tag changed before cleanup"
        );
        docker_run(&["image", "rm", &self.tag]);
        require_absent_tag(&self.tag);
    }
}

impl Drop for OwnedTag {
    fn drop(&mut self) {
        let inspect = std::process::Command::new("docker")
            .args(["image", "inspect", "--format", "{{.Id}}", &self.tag])
            .output();
        if let Ok(output) = inspect
            && output.status.success()
            && String::from_utf8_lossy(&output.stdout).trim() == self.image_id
        {
            let result = std::process::Command::new("docker")
                .args(["image", "rm", &self.tag])
                .output();
            if !result.is_ok_and(|output| output.status.success()) {
                eprintln!("could not clean up test-owned Docker tag {}", self.tag);
            }
        }
    }
}

fn require_absent_tag(tag: &str) {
    let tags = docker_run(&["image", "ls", "--format", "{{.Repository}}:{{.Tag}}"]);
    assert!(
        !tags.lines().any(|existing| existing == tag),
        "test requires an unused derived tag: {tag}"
    );
}

#[test]
fn squat_fixture_recipes_do_not_share_the_common_composite_tags() {
    let lock = ImageLock::bundled().unwrap();
    let recipe = |tools: &[&str]| {
        let tools = tools
            .iter()
            .map(|tool| (*tool).to_owned())
            .collect::<Vec<_>>();
        match lock.plan_project_runtime("amd64", &tools).unwrap() {
            ProjectRuntimeImagePlan::Composition(plan) => plan.recipe_sha256,
            _ => panic!("fixture tool set must require a composition"),
        }
    };
    let refusal = recipe(&["python", "go", "node", "npm"]);
    let recovery = recipe(&["cargo", "rustc", "go"]);
    assert_ne!(refusal, recipe(&["python", "go"]));
    assert_ne!(recovery, recipe(&["cargo", "go"]));
    assert_ne!(refusal, recovery);
}

/// Production boundary: panic cleanup may remove only a tag still bound to
/// the fixture's image. Parser tests cannot prove the Docker command boundary.
/// Uses a temporary fake Docker CLI, no daemon/network, negligible setup.
#[cfg(unix)]
#[test]
fn squat_tag_cleanup_preserves_a_replaced_tag_even_during_unwind() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let docker = temp.path().join("docker");
    let state = temp.path().join("state");
    let log = temp.path().join("log");
    std::fs::write(&docker, "#!/bin/sh\nif [ \"$2\" = inspect ]; then cat \"$KOOLADE_TEST_IMAGE_STATE\"; exit $?; fi\nif [ \"$2\" = rm ]; then printf '%s\\n' \"$3\" >> \"$KOOLADE_TEST_IMAGE_LOG\"; rm \"$KOOLADE_TEST_IMAGE_STATE\"; exit $?; fi\nexit 1\n").unwrap();
    std::fs::set_permissions(&docker, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    let mut paths = vec![temp.path().to_owned()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    environment.set("PATH", std::env::join_paths(paths).unwrap());
    environment.set("KOOLADE_TEST_IMAGE_STATE", &state);
    environment.set("KOOLADE_TEST_IMAGE_LOG", &log);
    std::fs::write(&state, "sha256:owned\n").unwrap();
    let _ = std::panic::catch_unwind(|| {
        let _guard = OwnedTag {
            tag: "fixture:tag".into(),
            image_id: "sha256:owned".into(),
        };
        panic!("synthetic interruption");
    });
    assert_eq!(std::fs::read_to_string(&log).unwrap(), "fixture:tag\n");
    std::fs::write(&state, "sha256:foreign\n").unwrap();
    let _ = std::panic::catch_unwind(|| {
        let _guard = OwnedTag {
            tag: "fixture:tag".into(),
            image_id: "sha256:owned".into(),
        };
        panic!("synthetic interruption");
    });
    assert_eq!(std::fs::read_to_string(&state).unwrap(), "sha256:foreign\n");
    assert_eq!(std::fs::read_to_string(&log).unwrap(), "fixture:tag\n");
}

/// Planted squats on the app-owned derived tag namespace must heal
/// non-destructively. Production boundary: the Docker daemon's persistent
/// tag storage is adversarial input to every composition. Covers the two
/// dispositions the engine exposes to operators:
///
/// * an unverified foreign image squatting the tag under a single tag — the
///   exact posture ordinary `docker import` creates, because the engine mints
///   a digest reference that expires with the last tag and therefore offers
///   no real protection — is refused with an operator-actionable error and
///   survives untouched;
/// * a signed library image squatting the tag under an alias keeps its own
///   references, so the recovery path likewise releases the reservation and
///   rebuilds without touching the library.
///
/// This scenario exercises the tag-only protection rule in
/// `derived_tag_guard`: repo digest references never count as protection
/// because the Docker engine deletes an image when its last tag is removed.
///
/// These distinct tool sets and the rootless-Docker scheduling group avoid
/// collisions with other known scenarios in the opt-in suite.

#[test]
fn planted_derived_tag_squats_recover_non_destructively_and_preserve_foreign_data() {
    if !opted_in_on_linux() {
        return;
    }
    let lock = ImageLock::bundled().expect("reviewed runtime image lock");
    let arch = native_docker_arch();
    anchor_tag_less_foundations();
    let fixture = Fixture::new();
    let (workspace, task_clone) = committed_workspace(&fixture);

    // Scenario A — foreign squat: an unverified image plants itself under
    // the content-addressed derived tag for python+go+node+npm.
    let refusal_tools = vec![
        "python".to_owned(),
        "go".to_owned(),
        "node".to_owned(),
        "npm".to_owned(),
    ];
    let refusal_hash = match lock.plan_project_runtime(arch, &refusal_tools) {
        Ok(ProjectRuntimeImagePlan::Composition(candidate)) => candidate.recipe_sha256,
        other => panic!("precondition: python+go+node+npm must plan a composition, got {other:?}"),
    };
    let reserved_tag = format!("koolade-derived:{}", strip_sha(&refusal_hash));
    require_absent_tag(&reserved_tag);
    let squatter_dir = tempfile::tempdir().expect("scratch import dir");
    let squatter_payload = squatter_dir.path().join("marker");
    std::fs::write(&squatter_payload, uuid::Uuid::new_v4().to_string())
        .expect("scribble import payload");
    let squatter_tar = squatter_dir.path().join("payload.tar");
    let packed = std::process::Command::new("tar")
        .arg("cf")
        .arg(&squatter_tar)
        .arg("-C")
        .arg(squatter_dir.path())
        .arg("marker")
        .output()
        .expect("pack squatter payload");
    assert!(packed.status.success(), "tar packing failed: {:?}", packed);
    let anchor = format!("koolade-test-squat:{}", uuid::Uuid::new_v4().simple());
    let _import_print = docker_run(&["import", squatter_tar.to_str().unwrap(), &anchor]);
    // The unique anchor establishes ownership independently of the derived
    // tag. A failed import/inspection can leave only this private test tag.
    let imported_id = normalized_image_id(&docker_run(&[
        "image", "inspect", "--format", "{{.Id}}", &anchor,
    ]));
    let anchor_guard = OwnedTag {
        tag: anchor,
        image_id: imported_id.clone(),
    };
    let imported_guard = OwnedTag {
        tag: reserved_tag.clone(),
        image_id: imported_id.clone(),
    };
    docker_run(&["tag", &imported_id, &reserved_tag]);
    anchor_guard.remove_checked();
    drop(anchor_guard);
    let protected_counters = reference_counters(&imported_id);
    assert!(
        protected_counters.digest_count >= 1,
        "ordinary image creation mints a digest reference that the engine lets expire with the last tag"
    );
    // The reserved tag is thus the squatter's only protection; the builder
    // must refuse to release it and keep the foreign image intact.
    let refusal = match ExecutionSession::connect_local_with_tools(
        workspace.clone(),
        &task_clone,
        ExecutionModeV1::Implementation,
        &refusal_tools,
        ResourceBudgetV1::default(),
    ) {
        Err(refusal) => refusal,
        Ok(_) => panic!("a squatting single-tag image must be refused, not deleted"),
    };
    assert!(
        refusal.message.contains("reconcile the tag manually"),
        "operator-actionable guidance expected, got: {}",
        refusal.message
    );
    let survivor = docker_run(&["image", "inspect", "--format", "{{.Id}}", &imported_id]);
    assert_eq!(
        survivor, imported_id,
        "the refused squatter image must survive untouched"
    );
    let still_planted = docker_run(&["image", "inspect", "--format", "{{.Id}}", &reserved_tag]);
    assert_eq!(
        still_planted, imported_id,
        "the planted tag must remain untouched after refusal"
    );
    // Scaffold teardown: only the test's own imported image is removed.
    imported_guard.remove_checked();
    drop(imported_guard);

    // Scenario B — signed library alias: the Rust foundation squats under
    // the cargo+rustc+go derived tag while keeping its library references.
    let recovery_tools = vec!["cargo".to_owned(), "rustc".to_owned(), "go".to_owned()];
    let recovery_hash = match lock.plan_project_runtime(arch, &recovery_tools) {
        Ok(ProjectRuntimeImagePlan::Composition(candidate)) => candidate.recipe_sha256,
        other => panic!("precondition: cargo+rustc+go must plan a composition, got {other:?}"),
    };
    let alien_profile = lock
        .select(RuntimeProfile::Rust, arch)
        .expect("locked profile image");
    let recovery_tag = format!("koolade-derived:{}", strip_sha(&recovery_hash));
    require_absent_tag(&recovery_tag);
    let alien_id = normalized_image_id(&docker_run(&[
        "image",
        "inspect",
        "--format",
        "{{.Id}}",
        &alien_profile.digest,
    ]));
    let _alien_guard = OwnedTag {
        tag: recovery_tag.clone(),
        image_id: alien_id,
    };
    docker_run(&["tag", &alien_profile.digest, &recovery_tag]);
    let recovered = ExecutionSession::connect_local_with_tools(
        workspace,
        &task_clone,
        ExecutionModeV1::Implementation,
        &recovery_tools,
        ResourceBudgetV1::default(),
    )
    .expect("aliased squat must be released and the recipe rebuilt");
    let recovered_digest = recovered.connected_image_digest();
    assert_ne!(
        recovered_digest, alien_profile.digest,
        "the connected runtime must be the freshly built composite"
    );
    let library_tags = docker_run(&[
        "image",
        "inspect",
        "--format",
        "{{range .RepoTags}}{{.}} {{end}}",
        &alien_profile.digest,
    ]);
    assert!(
        !library_tags.contains(strip_sha(&recovery_hash)),
        "only the app-owned derived tag may be released, got: {library_tags}"
    );
    let proof = recovered
        .run_tool_command("set -eu\ngo version", Duration::from_secs(180))
        .expect("scenario B composite executes go");
    assert_eq!(proof.result.exit_code, Some(0), "{:#?}", proof.result);
}
fn strip_sha(identity: &str) -> &str {
    identity.strip_prefix("sha256:").unwrap_or(identity)
}
