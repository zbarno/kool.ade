use super::*;

#[test]
fn start_outputs_are_scoped_to_the_selected_target() {
    let mut manifest = manifest();
    let mut recipe = recipe();
    recipe.allowed_output_paths = vec![".runtime/cache".into()];
    recipe.expected_artifacts = vec![".runtime/cache".into(), "dist/site.html".into()];
    manifest.recipes = vec![recipe];

    assert_eq!(
        super::super::super::source_output_paths(&manifest, &target(), "start-web").unwrap(),
        vec!["apps/.runtime/cache", "apps/dist/site.html"]
    );
}

#[test]
fn start_outputs_are_deduplicated_after_target_path_normalization() {
    let mut manifest = manifest();
    manifest.recipes[0].allowed_output_paths = vec!["./dist".into()];
    manifest.recipes[0].expected_artifacts = vec!["dist".into()];

    assert_eq!(
        super::super::super::source_output_paths(&manifest, &target(), "start-web").unwrap(),
        vec!["apps/dist"]
    );
}

#[test]
fn start_accepts_explicit_outputs_from_build_tests_and_start_recipes() {
    let mut build = recipe();
    build.recipe_id = "build-web".into();
    build.allowed_output_paths = vec!["dist".into()];
    build.expected_artifacts = vec!["dist/site.html".into()];
    let mut test = recipe();
    test.recipe_id = "test-web".into();
    test.allowed_output_paths = vec!["test-results".into()];
    test.expected_artifacts = vec!["test-results/report.xml".into()];
    let mut start = recipe();
    start.allowed_output_paths = vec![".runtime/cache".into()];

    let mut target = target();
    target.project_relative_root = "apps/web".into();
    target.build_recipe_ref = Some("build-web".into());
    target.test_recipe_refs = vec!["test-web".into()];
    let mut manifest = manifest();
    manifest.candidate_targets = vec![target.clone()];
    manifest.recipes = vec![build, test, start];

    assert_eq!(
        super::super::super::source_input_paths(
            &manifest,
            &target,
            RuntimeOperation::Start,
            "start-web",
        )
        .unwrap(),
        vec![
            "apps/web/.runtime/cache",
            "apps/web/dist",
            "apps/web/dist/site.html",
            "apps/web/test-results",
            "apps/web/test-results/report.xml"
        ]
    );
    assert_eq!(
        super::super::super::source_output_paths(&manifest, &target, "start-web").unwrap(),
        ["apps/web/.runtime/cache"]
    );
}
