use super::*;

#[test]
// Production boundary: the rendered checkbox selection is serialized into the
// AddRuntimeTarget command used for approval. Validator/store tests cannot
// prove egui wiring. Uses only an in-process egui context and synthetic IDs;
// no Docker, SSH, network, or credentials; setup cost is four pointer-clicks.
fn rendered_prepared_dependency_selection_reaches_target_command() {
    let context = egui::Context::default();
    let ticket = "fixture-task";
    let request_id = "prepared-request-fixture";
    let choices = [task_detail::PreparedDependencyChoice {
        request_id: request_id.into(),
        label: "python: fixture-package 1.2.3".into(),
    }];
    seed_test_repository_root_draft(&context, ticket, crate::project_runtime::AppSurface::Web);
    let mut command = None;
    let mut output = frame(&context, ticket, &choices, &mut command, Vec::new());
    click_text(
        &context,
        ticket,
        &choices,
        &mut command,
        &mut output,
        "Add a custom application target",
    );
    assert!(text_position(&output, "Runtime QA fixture reference (optional)").is_some());
    click_text(
        &context,
        ticket,
        &choices,
        &mut command,
        &mut output,
        "Build recipe",
    );
    assert!(text_position(&output, "Allowed environment variable names, one per line").is_some());
    click_text(
        &context,
        ticket,
        &choices,
        &mut command,
        &mut output,
        "Prepared dependencies for this target",
    );
    click_text(
        &context,
        ticket,
        &choices,
        &mut command,
        &mut output,
        "python: fixture-package 1.2.3",
    );
    click_text(
        &context,
        ticket,
        &choices,
        &mut command,
        &mut output,
        "Approve and save custom target",
    );

    let Some(crate::ui::ApplicationCommand::TaskDetail(task_detail::Command::AddRuntimeTarget {
        required_dependencies,
        fixture_ref,
        build,
        start,
        ..
    })) = command
    else {
        panic!("the rendered approval button emits AddRuntimeTarget");
    };
    assert_eq!(required_dependencies, [request_id]);
    assert_eq!(fixture_ref, "fixture-web-smoke");
    assert_eq!(build.allowed_env_keys_lines, "APP_BUILD_MODE");
    assert_eq!(start.allowed_env_keys_lines, "APP_RUNTIME_MODE");
}

#[test]
fn rendered_test_only_target_emits_empty_artifact_spec() {
    let context = egui::Context::default();
    let ticket = "fixture-test-only-task";
    seed_test_only_target_draft(&context, ticket);
    let mut command = None;
    let mut output = frame(&context, ticket, &[], &mut command, Vec::new());
    click_text(
        &context,
        ticket,
        &[],
        &mut command,
        &mut output,
        "Add a custom application target",
    );
    click_text(
        &context,
        ticket,
        &[],
        &mut command,
        &mut output,
        "Approve and save custom target",
    );

    let Some(crate::ui::ApplicationCommand::TaskDetail(task_detail::Command::AddRuntimeTarget {
        artifact_paths,
        build,
        test,
        start,
        ..
    })) = command
    else {
        panic!("Test-only custom targets can be approved without Build artifacts");
    };
    assert!(artifact_paths.trim().is_empty());
    assert!(build.executable_id.trim().is_empty());
    assert_eq!(test.executable_id, "python");
    assert!(start.executable_id.trim().is_empty());
}

fn click_text(
    context: &egui::Context,
    ticket: &str,
    choices: &[task_detail::PreparedDependencyChoice],
    command: &mut Option<crate::ui::ApplicationCommand>,
    output: &mut egui::FullOutput,
    text: &str,
) {
    let position = text_position(output, text).unwrap_or_else(|| panic!("{text} is rendered"));
    *output = frame(
        context,
        ticket,
        choices,
        command,
        vec![
            egui::Event::PointerMoved(position),
            pointer_button(position, true),
            pointer_button(position, false),
        ],
    );
}

fn frame(
    context: &egui::Context,
    ticket: &str,
    choices: &[task_detail::PreparedDependencyChoice],
    command: &mut Option<crate::ui::ApplicationCommand>,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    context.memory_mut(|memory| memory.set_everything_is_visible(true));
    let mut output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 1200.0),
            )),
            events,
            ..Default::default()
        },
        |context| {
            egui::CentralPanel::default().show(context, |ui| {
                paint_with_dispatch(ui, ticket, 1, choices, |next| *command = Some(next));
            });
        },
    );
    output.textures_delta.clear();
    output
}

fn text_position(output: &egui::FullOutput, text: &str) -> Option<egui::Pos2> {
    output.shapes.iter().find_map(|shape| {
        let egui::Shape::Text(text_shape) = &shape.shape else {
            return None;
        };
        (text_shape.galley.text() == text)
            .then(|| text_shape.pos + text_shape.galley.mesh_bounds.center().to_vec2())
    })
}

fn pointer_button(position: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: position,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    }
}
