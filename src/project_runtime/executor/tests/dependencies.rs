use super::super::require_prepared_dependencies;

#[test]
fn execution_requires_each_selected_broker_request_to_be_prepared_in_session() {
    let required = vec!["11111111-1111-4111-8111-111111111111".into()];
    let prepared = vec!["11111111-1111-4111-8111-111111111111".into()];
    assert!(require_prepared_dependencies(&required, &prepared).is_ok());
    assert!(require_prepared_dependencies(&required, &[]).is_err());
    assert!(require_prepared_dependencies(&[], &[]).is_ok());
}
