use super::*;

pub(super) fn settings(ctx: &egui::Context) {
    use crate::{app::dialogs::*, persistence::harness_settings::*};
    let mut settings = HarnessSettings {
        default_harness: Some("pi".into()),
        ..Default::default()
    };
    for (id, ready) in [("pi", true), ("codex", true), ("claude", false)] {
        settings.discovered.insert(
            id.into(),
            DetectedHarness {
                status: if ready {
                    "Ready to use"
                } else {
                    "Executable not found"
                }
                .into(),
                version: Some("review fixture".into()),
                executable: ready.then(|| format!("/opt/tools/{id}")),
                diagnostic: None,
                ready,
                models: vec!["project-default".into()],
                default_model: Some("project-default".into()),
                configuration_required: false,
                implementation_available: id == "pi" && ready,
            },
        );
    }
    settings.work_routes.insert(
        IMPLEMENTATION.into(),
        WorkRoute {
            harness: "pi".into(),
            model: Some("project-default".into()),
        },
    );
    let people = DlgSettings {
        user_name: "Example developer".into(),
        user_groups: "Platform, QA".into(),
        identity_note: "Seated as Example developer — project configuration".into(),
        rows: vec![
            Row {
                category: "Product".into(),
                members: "Product team".into(),
            },
            Row {
                category: "Security".into(),
                members: "Platform team".into(),
            },
        ],
        feedback: None,
    };
    ctx.data_mut(|data| {
        data.insert_temp(
            egui::Id::new("koolade_settings_harness_draft"),
            DlgHarnessSetup::review_fixture(settings),
        );
        data.insert_temp(egui::Id::new("koolade_settings_people_draft"), people);
    });
}

pub(super) fn planning(app: &mut KooladeApp) {
    let Screen::Connected(project) = &mut app.screen else {
        unreachable!()
    };
    let mut item = OpenItem::new(
        "CLR-010".into(),
        crate::domain::Priority::High,
        crate::domain::ItemKind::Question,
        "Product".into(),
        Some("Product team".into()),
        "Which users need access?".into(),
        "Determines the access model.".into(),
    );
    item.recommendation =
        "Start with invited team members and expand access after the first review.".into();
    project.state.items = vec![item];
    let mut work = crate::core::planning_work::Work::new(
        "planning-review".into(),
        "Plan a clearer team onboarding experience".into(),
        "Help new teams get started.".into(),
        "Choose the first workflow to support.".into(),
    );
    work.status = crate::core::planning_work::WorkStatus::NeedsAttention;
    project.planning_work = vec![work];
    let progress = project
        .activity
        .tasks
        .values()
        .next()
        .cloned()
        .unwrap_or_default();
    for key in ["CLR-010", "planning-review"] {
        project
            .activity
            .conversations
            .insert(key.into(), progress.clone());
    }
}

pub(super) fn feature(app: &mut KooladeApp) {
    use crate::domain::*;
    let Screen::Connected(project) = &mut app.screen else {
        unreachable!()
    };
    let source = "# CHG-001: Team onboarding\n\n## Intent\nHelp a new team reach its first useful project plan.\n\n## Requirements\n- Invite team members\n- Show clear progress\n- Preserve existing access\n";
    let identified =
        ArtifactIdentity::preserve_markdown(source, None, "CHG-001", "Team onboarding").unwrap();
    let identity = ArtifactIdentity::from_markdown(&identified)
        .unwrap()
        .unwrap();
    let body = ChangeMetadata::write_markdown(&identified, &identity, ChangeStatus::Ready).unwrap();
    project.state.active_features = vec![("CHG-001".into(), body)];
    let alternative = |id: &str, objective: &str| PlanAlternative {
        id: id.into(),
        objective: objective.into(),
        phases: vec![],
        files_touched: vec!["src/onboarding.rs".into()],
        state_changes: vec!["Save onboarding progress".into()],
        failure_modes: vec!["Interrupted setup can resume".into()],
        effort_band: "Small".into(),
        known_risks: vec!["Existing workspaces need a safe default".into()],
        reversibility: "Disable the new entry point".into(),
    };
    project.state.workflow.plan_comparisons.insert(
        "CHG-001".into(),
        crate::core::workflow::PlanComparisonRecord {
            schema_version: 1,
            feature_id: "CHG-001".into(),
            alternatives: PlanComparison {
                alternatives: vec![
                    alternative("A", "Guide the team through a short checklist."),
                    alternative("B", "Offer contextual tips as the team works."),
                ],
                recommendation: PlanRecommendation {
                    plan_id: "A".into(),
                    rationale: "A short checklist gives new teams a clear starting point.".into(),
                    evidence: vec!["Project onboarding requirements".into()],
                },
                selected_plan: None,
            },
            history: vec![],
            transcript: String::new(),
            status: crate::core::workflow::PlanComparisonStatus::Proposed,
            selected_plan: None,
            updated_at_ms: 0,
        },
    );
}
