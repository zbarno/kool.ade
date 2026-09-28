use super::*;

pub(super) fn save_evidence(
    scenario: Scenario,
    root: &Path,
    app: &PacketApp,
    completed: bool,
) -> serde_json::Value {
    let Screen::Connected(project) = &app.screen else {
        panic!("dogfood project was disconnected");
    };
    let saved = crate::core::planning_work::load(root).unwrap();
    let work = saved
        .iter()
        .find(|work| work.request == scenario.request)
        .or_else(|| saved.last())
        .unwrap();
    let state = crate::core::state::PlannerState::load(root).unwrap();
    let data = serde_json::json!({
        "scenario": scenario.code,
        "request": scenario.request,
        "provider": "local-vllm/qwen3.8-27b-fp8",
        "reasoning_level": std::env::var("PACKET_DOGFOOD_REASONING")
            .unwrap_or_else(|_| "off".into()),
        "turn_completed": completed,
        "planning_work": {
            "uid": work.uid,
            "kind": work.kind,
            "status": work.status,
            "title": work.title,
            "detail": work.detail,
            "feature_id": work.feature_id,
            "follow_up_task": work.follow_up_task,
        },
        "related_planning_work": saved.iter().filter(|other| other.uid != work.uid).collect::<Vec<_>>(),
        "features": state.active_features,
        "open_items": state.items,
        "resolved_items": state.resolved_items,
        "product_specification": state.spec_text,
        "workflow_ready": state.workflow.ready(state.planning_contract()),
        "conversation": project.chat.iter().map(|message| serde_json::json!({
            "role": message.role,
            "text": message.text,
        })).collect::<Vec<_>>(),
        "board_attention_ids": project.state.items.iter()
            .filter(|item| matches!(item.authority, crate::domain::Authority::Human | crate::domain::Authority::Review))
            .map(|item| item.id.clone()).collect::<Vec<_>>(),
    });
    let evidence = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".kool-ade-packet/planning/tasks/packet-task-centric-ux-remediation/evidence")
        .join(format!(
            "scenario-{}-live.json",
            scenario.code.to_ascii_lowercase()
        ));
    std::fs::create_dir_all(evidence.parent().unwrap()).unwrap();
    std::fs::write(&evidence, serde_json::to_vec_pretty(&data).unwrap()).unwrap();
    data
}

pub(super) fn continue_scenario_a(
    app: &mut PacketApp,
    ctx: &egui::Context,
    root: &Path,
) -> serde_json::Value {
    let Screen::Connected(project) = &app.screen else {
        panic!("dogfood project was disconnected");
    };
    let item = project.state.items.iter().find(|item| {
        matches!(item.authority, crate::domain::Authority::Human) && item.feature_id.is_some()
    });
    let scripted_choice = if let Some(item) = item {
        let item_id = item.id.clone();
        let choice = item
            .decision_brief
            .as_ref()
            .and_then(|brief| brief.recommendation.as_ref())
            .map(|recommendation| recommendation.option_id.clone())
            .unwrap_or_else(|| panic!("Scenario A choice needs an advisory recommendation"));

        // This is a scripted operator choice in an isolated dogfood repository,
        // not an adopted decision in the user's project.
        *app.task_draft(&item_id).unwrap() = format!(
            "For this isolated dogfood run, I choose Packet's recommended option `{choice}`."
        );
        app.submit_task_reply(&item_id);
        assert!(
            app.task_chat_active(&item_id),
            "board answer starts planning"
        );
        assert!(
            complete_live_task_turn(app, ctx, &item_id),
            "Scenario A board answer turn completes"
        );
        Some(serde_json::json!({"item_id": item_id, "recommended_option": choice}))
    } else {
        None
    };

    let feature_id = match &app.screen {
        Screen::Connected(project) => project
            .state
            .active_feature
            .as_ref()
            .map(|(id, _)| id.clone())
            .unwrap_or_else(|| panic!("Scenario A feature remains active after answer")),
        _ => panic!("dogfood project was disconnected"),
    };
    let Screen::Connected(project) = &app.screen else {
        unreachable!()
    };
    let feature = project
        .state
        .active_features
        .iter()
        .find(|(id, _)| id == &feature_id)
        .expect("Scenario A feature remains saved after answer");
    assert_eq!(
        crate::domain::ChangeMetadata::require_markdown(&feature.1)
            .unwrap()
            .status,
        crate::domain::ChangeStatus::Ready,
        "Scenario A must reach Ready before the approval action"
    );
    assert!(
        project
            .state
            .items
            .iter()
            .all(|open| open.feature_id.as_deref() != Some(&feature_id)),
        "Scenario A must resolve its blocking item before approval"
    );
    app.dispatch_ui_command(crate::ui::ApplicationCommand::ApproveFeature {
        id: feature_id.clone(),
    });
    assert!(
        app.feature_approved(&feature_id),
        "board approval is recorded"
    );
    assert!(
        complete_live_turn(app, ctx),
        "Scenario A task preparation completes"
    );

    let evidence = save_evidence(
        Scenario {
            code: "A",
            kind: "Feature",
            request: "Add the ability to export planning tasks as Markdown.",
        },
        root,
        app,
        true,
    );
    serde_json::json!({
        "scripted_operator_choice": scripted_choice,
        "choice_source": "advisory recommendation in isolated dogfood fixture; null when no choice was needed",
        "feature_id": feature_id,
        "approval_recorded": app.feature_approved(&feature_id),
        "final_evidence": evidence,
    })
}

pub(super) fn approve_and_prepare_scenario_a(
    app: &mut PacketApp,
    ctx: &egui::Context,
    root: &Path,
) -> serde_json::Value {
    let feature_id = match &app.screen {
        Screen::Connected(project) => project
            .state
            .active_feature
            .as_ref()
            .map(|(id, _)| id.clone())
            .expect("resumed Scenario A has an active feature"),
        _ => panic!("dogfood project was disconnected"),
    };
    let batch_already_persisted = matches!(&app.screen, Screen::Connected(project) if !project.state.workflow.task_batches.is_empty());
    let action_label = if batch_already_persisted {
        "Task batch already persisted by the preceding live generation turn".to_owned()
    } else {
        let action = app
            .feature_actions(None)
            .into_iter()
            .find(|action| action.id == feature_id)
            .expect("Ready Scenario A must expose the feature approval card");
        assert!(
            action.label().contains("Approve") || action.label().contains("Prepare tasks"),
            "Ready feature must expose an approval or task-preparation action"
        );
        let label = action.label();
        app.dispatch_ui_command(crate::ui::ApplicationCommand::ApproveFeature {
            id: feature_id.clone(),
        });
        label
    };
    assert!(
        app.feature_approved(&feature_id),
        "board approval is recorded"
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(600);
    while !matches!(&app.screen, Screen::Connected(project) if !project.state.workflow.task_batches.is_empty())
    {
        app.tick(0.016, ctx);
        if std::time::Instant::now() >= deadline {
            panic!("Scenario A task preparation timed out; partial artifacts are preserved");
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let task_dir = crate::artifacts::repo_artifact(root, ".kool-ade-packet/planning/tasks");
    assert!(
        std::fs::read_dir(&task_dir).is_ok_and(|mut entries| entries.next().is_some()),
        "approved Scenario A must generate a task batch"
    );
    let history_path = root.join("runtime/task-conversations.json");
    let histories: serde_json::Value = std::fs::read(&history_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    let answer_history = histories
        .pointer("/otherHistories/CLR-001")
        .cloned()
        .unwrap_or_else(|| serde_json::json!([]));
    let batch_count = match &app.screen {
        Screen::Connected(project) => project.state.workflow.task_batches.len(),
        _ => 0,
    };
    serde_json::json!({
        "feature_id": feature_id,
        "approval_action": action_label,
        "approval_recorded": app.feature_approved(&feature_id),
        "scripted_operator_choice_and_response_history": answer_history,
        "persisted_task_batch_count": batch_count,
        "planning_task_batch_created": true,
        "final_evidence": save_evidence(
            Scenario { code: "A", kind: "Feature", request: "Add the ability to export planning tasks as Markdown." },
            root,
            app,
            true,
        ),
    })
}
