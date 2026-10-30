use super::paint;
use crate::{
    app::{root::KooladeApp, welcome::onboarding::Step},
    core::managed_workspace::{ExistingProject, ExistingProjectKind},
};

#[test]
fn existing_project_places_a_clear_open_action_within_narrow_screen() {
    // Production egui painter and clipping boundary; a state-only assertion
    // cannot prove the primary action is fully visible. No GPU or services.
    let mut app = KooladeApp::default();
    app.onboarding.step = Step::ExistingProject;
    app.onboarding.existing = Some(ExistingProject {
        folder: "/synthetic/code/kool.ade".into(),
        planning_repo: Some("/synthetic/code/kool.ade.packet".into()),
        workspace_id: uuid::Uuid::new_v4(),
        repository_id: "synthetic-code".into(),
        title: "Koolade".into(),
        create_workspace: false,
    });
    app.onboarding_project_kind = Some(ExistingProjectKind::Tracked);
    let ctx = egui::Context::default();
    paint(&mut app, &ctx, vec![]);
    let output = paint(&mut app, &ctx, vec![]);
    let action_label = output.shapes.iter().find_map(|clipped| {
        let egui::Shape::Text(text) = &clipped.shape else {
            return None;
        };
        (text.galley.text() == "Connect and open project").then_some((
            text.galley.mesh_bounds.translate(text.pos.to_vec2()),
            clipped.clip_rect,
            text.galley.elided,
        ))
    });
    let visible_text = output
        .shapes
        .iter()
        .filter_map(|clipped| {
            let egui::Shape::Text(text) = &clipped.shape else {
                return None;
            };
            Some(text.galley.text().to_owned())
        })
        .collect::<Vec<_>>();
    let (label_rect, label_clip, elided) = action_label.unwrap_or_else(|| {
        panic!("open action label is not rendered; visible text: {visible_text:?}")
    });
    let button = output
        .shapes
        .iter()
        .filter_map(|clipped| {
            let egui::Shape::Rect(rect) = &clipped.shape else {
                return None;
            };
            rect.rect
                .contains_rect(label_rect)
                .then_some((rect.rect, clipped.clip_rect))
        })
        .min_by(|(left, _), (right, _)| left.area().total_cmp(&right.area()))
        .unwrap_or_else(|| panic!("button background does not contain label {label_rect:?}"));
    let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(360.0, 640.0));
    assert!(
        viewport.contains_rect(button.0),
        "button {:?} is outside viewport {:?}",
        button.0,
        viewport
    );
    assert!(
        button.1.contains_rect(button.0) && label_clip.contains_rect(button.0) && !elided,
        "button {:?} is clipped by {:?} or its label is elided",
        button.0,
        button.1
    );
}

#[test]
fn existing_project_can_open_when_setup_progress_cannot_be_saved() {
    use super::super::support::StateHome;

    // The open button is an explicit approval for this existing folder. A
    // damaged navigation-state file must not strand the user on a disabled
    // action; the durable workspace attachment remains the source of truth.
    let root = tempfile::tempdir().unwrap();
    let mut state_home = StateHome::new();
    state_home.set(root.path());
    let mut app = KooladeApp::default();
    app.onboarding.step = Step::ExistingProject;
    app.onboarding.existing = Some(ExistingProject {
        folder: root.path().join("synthetic-code"),
        planning_repo: Some(root.path().join("synthetic-planning")),
        workspace_id: uuid::Uuid::new_v4(),
        repository_id: "synthetic-code".into(),
        title: "Synthetic project".into(),
        create_workspace: false,
    });
    app.onboarding_project_kind = Some(ExistingProjectKind::Tracked);
    app.onboarding.error = Some("saved progress is unavailable".into());
    let ctx = egui::Context::default();
    paint(&mut app, &ctx, vec![]);
    let output = paint(&mut app, &ctx, vec![]);
    let position = output
        .shapes
        .iter()
        .find_map(|shape| {
            let egui::Shape::Text(text) = &shape.shape else {
                return None;
            };
            (text.galley.text() == "Connect and open project")
                .then(|| text.pos + text.galley.mesh_bounds.center().to_vec2())
        })
        .expect("selected code and planning folders show an open action");
    for pressed in [true, false] {
        paint(
            &mut app,
            &ctx,
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
        );
    }
    let worker = app
        .onboarding_creation
        .take()
        .expect("the explicit open action remains enabled");
    let _ = worker.join().unwrap();
}

#[test]
fn failed_folder_check_can_be_retried_from_the_pinned_action() {
    use super::super::support::StateHome;

    let root = tempfile::tempdir().unwrap();
    let mut state_home = StateHome::new();
    state_home.set(root.path());
    let mut app = KooladeApp::default();
    app.onboarding.step = Step::ExistingProject;
    app.onboarding.existing = Some(ExistingProject {
        folder: root.path().join("synthetic-code"),
        planning_repo: Some(root.path().join("synthetic-planning")),
        workspace_id: uuid::Uuid::new_v4(),
        repository_id: "synthetic-code".into(),
        title: "Synthetic project".into(),
        create_workspace: false,
    });
    app.conn_error = Some("folder check failed".into());
    let ctx = egui::Context::default();
    paint(&mut app, &ctx, vec![]);
    let output = paint(&mut app, &ctx, vec![]);
    let position = output
        .shapes
        .iter()
        .find_map(|shape| {
            let egui::Shape::Text(text) = &shape.shape else {
                return None;
            };
            (text.galley.text() == "Try code folder check again")
                .then(|| text.pos + text.galley.mesh_bounds.center().to_vec2())
        })
        .expect("failed inspection exposes a pinned retry action");
    for pressed in [true, false] {
        paint(
            &mut app,
            &ctx,
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
        );
    }
    let worker = app.onboarding_inspection.take();
    assert!(
        worker.is_some() || app.conn_error.as_deref() != Some("folder check failed"),
        "retry action either starts inspection or reports its result"
    );
    if let Some(worker) = worker {
        app.apply_onboarding_inspection(worker.join().unwrap());
    }
    assert_ne!(app.conn_error.as_deref(), Some("folder check failed"));
}

#[test]
fn new_planning_workspace_stays_blocked_when_setup_state_cannot_be_saved() {
    use super::super::support::StateHome;

    let root = tempfile::tempdir().unwrap();
    let mut state_home = StateHome::new();
    state_home.set(root.path());
    let mut app = KooladeApp::default();
    app.onboarding.step = Step::ExistingProject;
    app.onboarding.existing = Some(ExistingProject {
        folder: root.path().join("synthetic-code"),
        planning_repo: None,
        workspace_id: uuid::Uuid::new_v4(),
        repository_id: "synthetic-code".into(),
        title: "Synthetic project".into(),
        create_workspace: true,
    });
    app.onboarding_project_kind = Some(ExistingProjectKind::Tracked);
    app.onboarding.error = Some("saved progress is unavailable".into());
    let ctx = egui::Context::default();
    paint(&mut app, &ctx, vec![]);
    let output = paint(&mut app, &ctx, vec![]);
    let position = output
        .shapes
        .iter()
        .find_map(|shape| {
            let egui::Shape::Text(text) = &shape.shape else {
                return None;
            };
            (text.galley.text() == "Connect and open project")
                .then(|| text.pos + text.galley.mesh_bounds.center().to_vec2())
        })
        .expect("new workspace action remains visible with an explanation");
    for pressed in [true, false] {
        paint(
            &mut app,
            &ctx,
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
        );
    }
    assert!(app.onboarding_creation.is_none());
}
