#[cfg(target_os = "linux")]
use std::sync::Arc;

/// App-owned Runtime QA authority. It carries the existing consent broker host
/// and controller; a QA build approval alone never creates this capability.
#[cfg(target_os = "linux")]
#[derive(Clone)]
pub(in crate::app::root::tick::activity) struct ComputerQaRuntimeContextV1 {
    pub host: Arc<dyn crate::computer_use::mcp::ComputerUseMcpHostV1>,
    pub controller: Arc<crate::computer_use::mcp::ComputerUseMcpControllerV1>,
    pub project_id: String,
    pub user_id: String,
}

#[cfg(not(target_os = "linux"))]
#[derive(Clone)]
pub(in crate::app::root::tick::activity) struct ComputerQaRuntimeContextV1;

#[cfg(target_os = "linux")]
impl ComputerQaRuntimeContextV1 {
    pub fn caller(
        &self,
        task_id: &str,
        session_id: &str,
    ) -> crate::computer_use::mcp::ComputerUseCallerV1 {
        crate::computer_use::mcp::ComputerUseCallerV1 {
            identity: crate::computer_use::ComputerIdentityV1 {
                project_id: self.project_id.clone(),
                task_id: task_id.to_owned(),
                user_id: self.user_id.clone(),
                session_id: session_id.to_owned(),
            },
            actor: crate::computer_use::ComputerActionActorV1::Human,
            role_id: crate::computer_use::mcp::COMPUTER_USE_RUNTIME_QA_ROLE_V1.to_owned(),
        }
    }
}
