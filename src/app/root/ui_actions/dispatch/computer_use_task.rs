use super::super::super::KooladeApp;
use crate::ui::ComputerUseTaskActionV1 as Action;

pub(super) fn dispatch(app: &mut KooladeApp, task_id: &str, session_id: &str, action: Action) {
    #[cfg(target_os = "linux")]
    {
        let uid = unsafe { libc::geteuid() };
        let user_id = uuid::Uuid::new_v5(
            &uuid::Uuid::NAMESPACE_OID,
            format!("koolade-linux-effective-uid-v1:{uid}").as_bytes(),
        )
        .to_string();
        let controller = app.shared_computer_use_controller();
        let result = match action {
            Action::PauseAgent => controller.queue_control_change(
                task_id,
                &user_id,
                session_id,
                crate::computer_use::mcp::ComputerUseControlCommandV1::PauseAgent,
            ),
            Action::ResumeAgent => controller.queue_control_change(
                task_id,
                &user_id,
                session_id,
                crate::computer_use::mcp::ComputerUseControlCommandV1::ResumeAgent,
            ),
            Action::TakeControl => controller.queue_control_change(
                task_id,
                &user_id,
                session_id,
                crate::computer_use::mcp::ComputerUseControlCommandV1::TakeControl,
            ),
            Action::ReturnControl => controller.queue_control_change(
                task_id,
                &user_id,
                session_id,
                crate::computer_use::mcp::ComputerUseControlCommandV1::ReturnControl,
            ),
            Action::StopComputer => controller.queue_control_change(
                task_id,
                &user_id,
                session_id,
                crate::computer_use::mcp::ComputerUseControlCommandV1::StopComputer,
            ),
            Action::InspectUncertainOutcome => controller.queue_control_change(
                task_id,
                &user_id,
                session_id,
                crate::computer_use::mcp::ComputerUseControlCommandV1::InspectUncertainOutcome,
            ),
            Action::SetPrivateInput(enabled) => {
                controller.set_private_input(task_id, &user_id, session_id, enabled)
            }
            Action::DisplayInput(input) => {
                controller.send_human_display_input(task_id, &user_id, session_id, input)
            }
            Action::LaunchApprovedApp(app_id) => controller.queue_control_change(
                task_id,
                &user_id,
                session_id,
                crate::computer_use::mcp::ComputerUseControlCommandV1::LaunchApprovedApp(app_id),
            ),
            Action::ManageApprovedApp { app_id, operation } => controller.queue_control_change(
                task_id,
                &user_id,
                session_id,
                crate::computer_use::mcp::ComputerUseControlCommandV1::ManageApprovedApp {
                    app_id,
                    operation,
                },
            ),
        };
        if let Err(error) = result {
            app.toasts
                .warning(format!("Computer Use control was not applied: {error}."));
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (app, task_id, session_id, action);
    }
}
