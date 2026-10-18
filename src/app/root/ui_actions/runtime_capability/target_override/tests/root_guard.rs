use super::super::super::Screen;
use crate::project_runtime::AppSurface;

#[test]
fn controller_rejects_traversal_root_before_loading_task_state() {
    let mut app = crate::app::root::board_tests::fixture();
    let Screen::Connected(project) = &mut app.screen else {
        panic!("fixture is connected")
    };
    let error = super::super::add(
        project,
        "missing-task",
        AppSurface::Web,
        "../outside",
        "dist/index.html",
        "",
        &crate::ui::task_detail::RuntimeRecipeInput::default(),
        &crate::ui::task_detail::RuntimeRecipeInput::default(),
        &crate::ui::task_detail::RuntimeRecipeInput::default(),
        crate::ui::layout::RuntimeTargetApprovalProof::for_test(),
        "",
        "/",
        0,
    )
    .unwrap_err();
    assert!(error.contains("repository-relative"));
}

#[test]
fn controller_rejects_noncanonical_or_duplicate_artifacts_before_task_lookup() {
    let mut app = crate::app::root::board_tests::fixture();
    let Screen::Connected(project) = &mut app.screen else {
        panic!("fixture is connected")
    };
    let error = super::super::add(
        project,
        "missing-task",
        AppSurface::Web,
        "./",
        "./dist/index.html\ndist/index.html",
        "",
        &crate::ui::task_detail::RuntimeRecipeInput::default(),
        &crate::ui::task_detail::RuntimeRecipeInput::default(),
        &crate::ui::task_detail::RuntimeRecipeInput::default(),
        crate::ui::layout::RuntimeTargetApprovalProof::for_test(),
        "",
        "/",
        0,
    )
    .unwrap_err();
    assert!(error.contains("canonical"));
}

#[test]
fn controller_rejects_control_characters_in_artifacts_before_task_lookup() {
    let mut app = crate::app::root::board_tests::fixture();
    let Screen::Connected(project) = &mut app.screen else {
        panic!("fixture is connected")
    };
    let error = super::super::add(
        project,
        "missing-task",
        AppSurface::Web,
        ".",
        "dist/\tindex.html",
        "",
        &crate::ui::task_detail::RuntimeRecipeInput::default(),
        &crate::ui::task_detail::RuntimeRecipeInput::default(),
        &crate::ui::task_detail::RuntimeRecipeInput::default(),
        crate::ui::layout::RuntimeTargetApprovalProof::for_test(),
        "",
        "/",
        0,
    )
    .unwrap_err();
    assert!(error.contains("artifact paths"));
}
