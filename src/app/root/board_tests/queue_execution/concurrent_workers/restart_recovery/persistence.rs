pub(super) fn persist_fixture_identities(
    store: &crate::artifacts::planning_store::PlanningStore,
    documents: &[crate::artifacts::task_docs::TaskDocument],
) {
    for doc in documents {
        let mut markdown = String::from_utf8(store.read(&doc.path).unwrap()).unwrap();
        assert!(
            crate::domain::ArtifactIdentity::from_markdown(&markdown)
                .unwrap()
                .is_none(),
            "fixture identity marker is inserted once"
        );
        let identity = serde_json::to_string(doc.identity.as_ref().unwrap()).unwrap();
        markdown.push_str(&format!("\n<!-- koolade-artifact-id:v1 {identity} -->\n"));
        store.atomic_write(&doc.path, markdown.as_bytes()).unwrap();
    }
}

pub(super) fn reload_task_documents(
    store: &crate::artifacts::planning_store::PlanningStore,
    paths: &[String],
) -> Vec<crate::artifacts::task_docs::TaskDocument> {
    paths
        .iter()
        .map(|path| {
            let markdown = String::from_utf8(store.read(path).unwrap()).unwrap();
            let identity = crate::domain::ArtifactIdentity::from_markdown(&markdown)
                .unwrap()
                .expect("persisted task identity");
            let metadata = crate::artifacts::task_docs::parse_metadata(&markdown)
                .unwrap()
                .expect("persisted task metadata");
            assert_eq!(identity.uid, metadata.uid);
            crate::artifacts::task_docs::TaskDocument {
                path: path.clone(),
                title: identity.title.clone(),
                text: markdown,
                identity: Some(identity),
                metadata: Some(metadata),
                task_state: None,
                metadata_error: None,
            }
        })
        .collect()
}

pub(super) fn assert_implementation_recovery(
    actual: &std::collections::BTreeMap<String, crate::core::implementation::Implementation>,
    saved: &[crate::core::implementation::Implementation],
    task_uids: &[String],
) {
    let git = |root: &std::path::Path, args: &[&str]| {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    };
    for (index, saved) in saved.iter().enumerate() {
        let restored = actual
            .get(&saved.ticket)
            .expect("reloaded implementation state");
        assert_eq!(
            restored.task_uid.as_deref(),
            Some(task_uids[index].as_str())
        );
        assert_eq!(restored.task_repositories, saved.task_repositories);
        assert_eq!(restored.base_commit, saved.base_commit);
        assert_eq!(restored.source_commit, saved.source_commit);
        assert_eq!(restored.branch, saved.branch);
        assert_eq!(
            restored.task_repository_kind,
            crate::core::implementation::TaskRepositoryKind::Clone
        );
        assert_eq!(restored.status, saved.status);
        assert_ne!(
            restored.status,
            crate::core::implementation::ImplementationStatus::Completed
        );
        assert!(
            git(&restored.task_repository, &["status", "--porcelain"])
                .contains("partial-progress.txt")
        );
    }
    assert_eq!(
        actual[&saved[2].ticket].status,
        crate::core::implementation::ImplementationStatus::WaitingToMerge
    );
}
