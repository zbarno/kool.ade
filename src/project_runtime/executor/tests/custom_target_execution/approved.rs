use crate::project_runtime::{ProjectRuntimeExecutor, ProjectRuntimeStore, RuntimeOperation};

use super::setup::build;

#[test]
// Production boundary: the executor verifies persisted user approval then
// routes a recipe through the approved ExecutionSession. Store tests cannot
// prove the authorization gate controls backend dispatch. Uses an in-memory
// Docker control-plane double only, with no daemon, SSH, network, or host CLI.
fn persisted_approved_custom_target_reaches_selected_execution_session() {
    let fixture = build();
    let restarted_planning = crate::artifacts::planning_store::PlanningStore::new(
        fixture.planning.project_id,
        &fixture.planning.root,
        fixture.planning.mode,
    );
    let store = ProjectRuntimeStore::new(&restarted_planning);
    let receipts = ProjectRuntimeExecutor::new(&store)
        .execute(
            &fixture.session,
            &fixture.target_id,
            RuntimeOperation::Build,
        )
        .unwrap();
    assert_eq!(receipts.len(), 1);
    assert_eq!(
        receipts[0].state,
        crate::project_runtime::RuntimeReceiptState::Passed
    );
    let request = fixture.backend.request();
    assert_eq!(request.argv, ["node", "build.js", "--release"]);
    assert_eq!(request.working_directory_relative, ".");
    assert_eq!(request.image_digest, fixture.digest);
    assert_eq!(
        request.target,
        crate::execution::contract::ExecutionTarget::local()
    );
    let policy = serde_json::to_value(&request.policy).unwrap();
    assert_eq!(policy["workspace_write_scope"]["root_relative"], ".");
    assert_eq!(
        policy["approved_recipe_command"]["argv"],
        serde_json::json!(["build.js", "--release"])
    );
}
