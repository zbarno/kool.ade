use super::*;

#[test]
fn sandbox_pip_restores_use_only_the_broker_wheelhouse() {
    let fixture = Fixture::new();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let body = create_body(
        &request(ExecutionModeV1::Implementation),
        &workspace,
        &identity(),
        &selected_image(),
    )
    .unwrap();

    let environment = body.env.unwrap();
    assert!(environment.contains(&"PIP_CONFIG_FILE=/dev/null".into()));
    assert!(environment.contains(&"PIP_NO_INDEX=1".into()));
    assert!(
        environment.contains(&"PIP_FIND_LINKS=/home/executor/.cache/koolade/pip/wheels".into())
    );
}
