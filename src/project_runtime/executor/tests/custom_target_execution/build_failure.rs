use crate::{
    project_runtime::{ProjectRuntimeExecutor, ProjectRuntimeStore, RuntimeOperation},
    project_runtime_capability::observed_build_capability_state,
};

use super::setup;

#[test]
fn failed_approved_build_is_persisted_and_projects_build_failed_for_its_source() {
    let fixture = setup::build();
    fixture.backend.set_exit_code(1);
    let store = ProjectRuntimeStore::new(&fixture.planning);

    let receipt = ProjectRuntimeExecutor::new(&store)
        .execute(
            &fixture.session,
            &fixture.target_id,
            RuntimeOperation::Build,
        )
        .unwrap()
        .remove(0);

    assert_eq!(receipt.operation, RuntimeOperation::Build);
    assert_eq!(receipt.task_id, fixture.task_id);
    assert_eq!(receipt.target_id, fixture.target_id);
    assert_eq!(receipt.source_sha, fixture.source_sha);
    assert_eq!(receipt.exit_code, Some(1));
    assert_eq!(
        receipt.state,
        crate::project_runtime::RuntimeReceiptState::Failed
    );

    let persisted = store.load_receipt(&receipt.receipt_id).unwrap().unwrap();
    assert_eq!(persisted, receipt);
    let target = store
        .load_manifest()
        .unwrap()
        .unwrap()
        .candidate_targets
        .into_iter()
        .find(|target| target.target_id == fixture.target_id)
        .unwrap();
    assert_eq!(
        observed_build_capability_state(
            &target,
            &fixture.task_id,
            &fixture.source_sha,
            std::slice::from_ref(&persisted),
        ),
        Some(crate::project_runtime::TargetCapabilityState::BuildFailed)
    );
}
