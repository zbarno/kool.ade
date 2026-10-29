use crate::project_runtime::{ExecutionRecipeV1, RuntimeOperation};

pub(super) fn validate_recipe_command_for_operation(
    operation: RuntimeOperation,
    recipe: &ExecutionRecipeV1,
) -> anyhow::Result<()> {
    if operation == RuntimeOperation::Start {
        return Ok(());
    }
    anyhow::ensure!(
        crate::project_runtime::is_logical_tool_id(&recipe.executable_id),
        "Build and Test recipes must use a logical tool ID; target-relative executable paths are only allowed for Start"
    );
    Ok(())
}
