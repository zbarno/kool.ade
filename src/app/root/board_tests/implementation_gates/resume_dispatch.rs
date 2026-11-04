use super::*;

#[test]
fn resume_dispatch_accepts_feature_named_workspace_story() {
    if !std::env::var("KOOLADE_DOCKER_BACKEND_INTEGRATION").is_ok_and(|value| value == "1") {
        eprintln!(
            "Skipping Docker-backed implementation dispatch; set KOOLADE_DOCKER_BACKEND_INTEGRATION=1 to run it"
        );
        return;
    }
    let _shield = crate::core::gitops::test_support::shield("resume-feature-ticket");
    let root = std::env::temp_dir().join(format!(
        "koolade-resume-dispatch-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let ticket = "planning/tasks/demo/CHG-003-TASK-verify.md";
    std::fs::create_dir_all(&root).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    crate::core::state::test_support::seed_published_product_document(&root).unwrap();
    let project_id = crate::core::state::test_support::project_id_for_code_root(&root);
    let local_root = root.with_file_name(format!(
        "{}-private-state",
        root.file_name().unwrap().to_string_lossy()
    ));
    let planning_store = crate::artifacts::planning_store::PlanningStore::for_code_repository(
        project_id,
        &local_root,
        &root,
    );
    let batch_identity = crate::domain::ArtifactIdentity::new("BATCH-DEMO", "Demo batch");
    let mut identity = crate::domain::ArtifactIdentity::new("TASK-VERIFY", "Verify workspace");
    identity.parent_uid = Some(batch_identity.uid.clone());
    let metadata =
        crate::artifacts::task_docs::TaskMetadata::new(&identity, "root", vec![]).unwrap();
    let mut workflow = crate::core::workflow::Workflow::default();
    workflow
        .task_batches
        .push(crate::core::workflow::TaskBatchRef {
            identity: Some(batch_identity),
            feature: "CHG-003".into(),
            directory: "planning/tasks/demo".into(),
            count: 1,
            created_at_ms: 1,
        });
    crate::artifacts::task_docs::save_workflow(&planning_store, &workflow).unwrap();
    let ticket_path = planning_store.local_root().join(ticket);
    std::fs::create_dir_all(ticket_path.parent().unwrap()).unwrap();
    let embedded = crate::artifacts::task_docs::embed("# Verify workspace\n", &metadata).unwrap();
    let identity_marker = serde_json::to_string(&identity).unwrap();
    std::fs::write(
        ticket_path,
        format!("{embedded}\n<!-- koolade-artifact-id:v1 {identity_marker} -->\n"),
    )
    .unwrap();
    let mut app = fixture();
    if let Screen::Connected(p) = &mut app.screen {
        let mut record = p.implementation_states.values().next().unwrap().clone();
        record.ticket = ticket.into();
        record.ticket_text = "# Verify workspace\n".into();
        record.task_uid = Some(identity.uid.clone());
        record.repository_id = Some("root".into());
        record.status = ImplementationStatus::Blocked;
        record.pr_url = None;
        p.state =
            crate::core::state::PlannerState::load_with_store(&root, &planning_store).unwrap();
        p.task_documents = vec![crate::artifacts::task_docs::TaskDocument {
            path: ticket.into(),
            title: "Verify workspace".into(),
            text: record.ticket_text.clone(),
            identity: Some(identity.clone()),
            metadata: Some(metadata),
            task_state: None,

            metadata_error: None,
        }];
        p.implementation_states = [(ticket.into(), record)].into();
        p.queue.blocked.insert(
            ticket.into(),
            crate::core::implementation::Failure::other("Previous failure"),
        );
        p.queue.recovery_attempts.insert(ticket.into(), 1);
    }
    let integration_enabled =
        std::env::var("KOOLADE_DOCKER_BACKEND_INTEGRATION").is_ok_and(|value| value == "1");
    let capabilities = crate::harness::runtime_capabilities::RuntimeCapabilities {
        planning_access: crate::harness::runtime_capabilities::PlanningAccess::SuppliedContextOnly,
        implementation: false,
    };
    if integration_enabled {
        app.implement_task(ticket.into());
    } else {
        // Keep the ordinary unit path independent of whether this developer
        // machine happens to have a usable Docker Engine.
        app.start_implementation_with_capabilities(ticket.into(), true, capabilities);
    }
    let Screen::Connected(p) = &mut app.screen else {
        panic!("disconnected")
    };
    if !integration_enabled {
        assert!(p.active_implementations.is_empty());
        assert!(p.queue.last_error.contains("Docker"));
        assert!(p.queue.blocked.contains_key(ticket));
        std::fs::remove_dir_all(root).unwrap();
        return;
    }
    assert!(
        p.active_implementations.contains_key(ticket),
        "{}",
        p.queue.last_error
    );
    assert!(!p.queue.blocked.contains_key(ticket));
    assert!(!p.queue.recovery_attempts.contains_key(ticket));
    let controller = p.active_implementations.remove(ticket).unwrap();
    controller.request_cancel();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if matches!(
            controller.poll(),
            Some(crate::core::implementation::Event::Done(_))
        ) {
            break;
        }
        assert!(Instant::now() < deadline, "fixture worker failed to stop");
        std::thread::sleep(Duration::from_millis(10));
    }
    // No remote is configured: this dispatch test cannot launch Pi or publish.
    std::fs::remove_dir_all(root).unwrap();
}
