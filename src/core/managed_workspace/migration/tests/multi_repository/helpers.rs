use std::{
    fs,
    path::{Path, PathBuf},
};

use super::super::support::{Fixture, git};
use crate::{
    artifacts::planning_store::{PlanningStore, paths},
    core::{
        planning_work::Work,
        project_repos::{ProjectManifest, Repository},
    },
};

pub(super) const ALPHA_REMOTE: &str = "https://example.invalid/alpha.git";

pub(super) fn register_alpha(fixture: &Fixture) {
    let store = PlanningStore::legacy_embedded(uuid::Uuid::nil(), &fixture.code);
    let manifest = ProjectManifest {
        repositories: vec![
            Repository {
                id: "root".into(),
                role: "Planning root".into(),
                remote: String::new(),
                display_name: None,
            },
            Repository {
                id: "alpha".into(),
                role: "Alpha code".into(),
                remote: ALPHA_REMOTE.into(),
                display_name: None,
            },
        ],
    };
    store
        .atomic_write(
            paths::PROJECT_MANIFEST,
            &serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
}

pub(super) fn create_secondary(fixture: &Fixture) -> PathBuf {
    let path = fixture.root.join("alpha");
    fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "--quiet", "--initial-branch=main"]);
    git(&path, &["config", "user.name", "Koolade Test"]);
    git(
        &path,
        &["config", "user.email", "koolade-test@example.invalid"],
    );
    git(&path, &["remote", "add", "origin", ALPHA_REMOTE]);
    copy_tree(
        &fixture.code.join(".koolade-packet"),
        &path.join(".koolade-packet"),
    );
    git(&path, &["add", "."]);
    git(
        &path,
        &["commit", "--quiet", "-m", "registered alpha planning"],
    );
    path
}

pub(super) fn append_unique_work(path: &Path) {
    let store = PlanningStore::legacy_embedded(uuid::Uuid::nil(), path);
    let mut work = crate::core::planning_work::load(&store).unwrap();
    work.push(Work::new(
        "MIG-002".into(),
        "Alpha implementation".into(),
        "Retain alpha planning".into(),
        "Unique record from alpha checkout".into(),
    ));
    store
        .atomic_write(
            paths::WORK,
            &serde_json::to_vec_pretty(&serde_json::json!({
                "schemaVersion": 3,
                "items": work,
            }))
            .unwrap(),
        )
        .unwrap();
    git(path, &["add", ".koolade-packet/state/work.json"]);
    git(
        path,
        &["commit", "--quiet", "-m", "add alpha planning work"],
    );
}

pub(super) fn change_existing_work(path: &Path) {
    let store = PlanningStore::legacy_embedded(uuid::Uuid::nil(), path);
    let mut work = crate::core::planning_work::load(&store).unwrap();
    work[0].request = "Conflicting record from alpha".into();
    store
        .atomic_write(
            paths::WORK,
            &serde_json::to_vec_pretty(&serde_json::json!({
                "schemaVersion": 3,
                "items": work,
            }))
            .unwrap(),
        )
        .unwrap();
    git(path, &["add", ".koolade-packet/state/work.json"]);
    git(
        path,
        &["commit", "--quiet", "-m", "conflict alpha planning work"],
    );
}

pub(super) fn change_feature_approval(path: &Path) {
    let store = PlanningStore::legacy_embedded(uuid::Uuid::nil(), path);
    let mut workflow = crate::artifacts::task_docs::load_workflow(&store).unwrap();
    workflow
        .approved_features
        .insert("F1".into(), format!("sha256:{}", "c".repeat(64)));
    store
        .atomic_write(
            paths::WORKFLOW,
            &serde_json::to_vec_pretty(&workflow).unwrap(),
        )
        .unwrap();
    git(path, &["add", ".koolade-packet/state/workflow.json"]);
    git(
        path,
        &["commit", "--quiet", "-m", "change feature approval"],
    );
}

fn copy_tree(source: &Path, target: &Path) {
    fs::create_dir_all(target).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let destination = target.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&path, &destination);
        } else {
            assert!(entry.file_type().unwrap().is_file());
            fs::copy(path, destination).unwrap();
        }
    }
}

pub(super) fn snapshot(root: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(root: &Path, path: &Path, files: &mut Vec<(String, Vec<u8>)>) {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                walk(root, &path, files);
            } else {
                files.push((
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    fs::read(path).unwrap(),
                ));
            }
        }
    }
    let mut files = Vec::new();
    walk(root, root, &mut files);
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}

pub(super) fn planning_repo_count(fixture: &Fixture) -> usize {
    fs::read_dir(fixture.root.join("operator/projects"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().join("planning-repo").exists())
        .count()
}
