use std::{fs, path::Path, process::Command};

pub(super) const PR_URL: &str = "https://github.com/fixture/repo/pull/42";

pub(super) fn bind_fixture_provider_remote(
    root: &Path,
    store: &crate::artifacts::planning_store::PlanningStore,
) -> RestoreEnvironmentVariable {
    let remote = "ssh://git@github.com/fixture/repo.git";
    git(root, &["remote", "set-url", "origin", remote]);
    let upload_pack = root.join("fake-ssh");
    fs::write(
        &upload_pack,
        format!(
            "#!/bin/sh\nexec git-upload-pack '{}'\n",
            root.join(".origin.git").display()
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&upload_pack, fs::Permissions::from_mode(0o700)).unwrap();
    let restore_ssh = RestoreEnvironmentVariable::new("GIT_SSH_COMMAND");
    unsafe { std::env::set_var("GIT_SSH_COMMAND", upload_pack) };
    let manifest = crate::core::project_repos::ProjectManifest {
        repositories: vec![crate::core::project_repos::Repository {
            id: "root".into(),
            role: "Planning root".into(),
            remote: remote.into(),
            display_name: None,
        }],
    };
    store
        .atomic_write(
            "config/repositories.json",
            &serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
    restore_ssh
}

pub(super) struct RestoreEnvironmentVariable {
    name: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl RestoreEnvironmentVariable {
    fn new(name: &'static str) -> Self {
        Self {
            name,
            previous: std::env::var_os(name),
        }
    }
}

impl Drop for RestoreEnvironmentVariable {
    fn drop(&mut self) {
        unsafe {
            match self.previous.take() {
                Some(value) => std::env::set_var(self.name, value),
                None => std::env::remove_var(self.name),
            }
        }
    }
}

pub(super) fn bind_task_to_ticket(root: &Path, task_uid: &str, planning_item_uid: &str) {
    let record_path = format!("state/lifecycle_tasks/{task_uid}.json");
    let path = root
        .join(crate::artifacts::layout::canonical::ROOT)
        .join(&record_path);
    let mut wrapper: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    wrapper["record"]["planning_item_uid"] = planning_item_uid.into();
    fs::write(path, serde_json::to_vec(&wrapper).unwrap()).unwrap();
}

pub(super) fn save_refresh_state(
    root: &Path,
    ticket: &str,
    task_uid: &str,
    base_sha: &str,
    head_sha: &str,
    gh: &Path,
) {
    let dir = crate::artifacts::layout::ArtifactLayout::new(root)
        .implementation_root()
        .join(crate::core::implementation::key_for_ticket(ticket));
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("state.json"),
        serde_json::to_vec(&serde_json::json!({
            "schemaVersion": 2,
            "ticket": ticket,
            "task_uid": task_uid,
            "ticket_text": "PR provider lifecycle fixture",
            "branch": "koolade/pr-refresh",
            "base": "main",
            "base_commit": base_sha,
            "task_repository": root,
            "status": "awaiting_review",
            "detail": "Awaiting provider refresh",
            "pr_url": PR_URL,
            "verified_head": head_sha,
            "repository_id": "root",
        }))
        .unwrap(),
    )
    .unwrap();
    write_provider_response(gh, "OPEN", base_sha, head_sha, "");
}

pub(super) fn write_provider_response(gh: &Path, state: &str, base: &str, head: &str, merge: &str) {
    let merge_commit = if state == "MERGED" {
        serde_json::json!({"oid": merge})
    } else {
        serde_json::Value::Null
    };
    let merged_at = if state == "MERGED" {
        serde_json::Value::String("2026-10-18T10:00:00Z".into())
    } else {
        serde_json::Value::Null
    };
    let payload = serde_json::json!({
        "state": state,
        "mergeCommit": merge_commit,
        "mergedAt": merged_at,
        "number": 42,
        "headRefOid": head,
        "baseRefOid": base,
        "baseRefName": "main"
    });
    fs::write(gh, format!("#!/bin/sh\nprintf '%s\\n' '{}'\n", payload)).unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(gh, fs::Permissions::from_mode(0o700)).unwrap();
}

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
}
