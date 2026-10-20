use super::super::super::KooladeApp;
use crate::ui::task_detail::Command as TaskDetailCommand;

pub(super) fn dispatch(app: &mut KooladeApp, command: TaskDetailCommand) {
    match command {
        TaskDetailCommand::UpdateDraft { ticket, draft } => {
            if let crate::app::root::Screen::Connected(project) = &mut app.screen {
                project.task_chats.drafts.insert(ticket, draft);
            }
        }
        TaskDetailCommand::SubmitReply {
            ticket,
            draft,
            decision,
        } => {
            if let crate::app::root::Screen::Connected(project) = &mut app.screen {
                project.task_chats.drafts.insert(ticket.clone(), draft);
            }
            if decision {
                app.submit_implementation_decision(&ticket);
            } else {
                app.submit_task_reply(&ticket);
            }
        }
        TaskDetailCommand::RetryChatSave => app.retry_task_chat_save(),
        TaskDetailCommand::RetryExplanation { ticket, detail } => {
            app.retry_attention(&ticket, &detail)
        }
        TaskDetailCommand::CancelTask { ticket } => app.cancel_board_work(&ticket),
        TaskDetailCommand::StartOrResume { ticket } => app.start_implementation(ticket, true),
        TaskDetailCommand::PublishLifecyclePr { ticket } => app.publish_lifecycle_pr(&ticket),
        TaskDetailCommand::ApproveSpecReconciliation {
            ticket,
            expected_revision,
            proposal_sha256,
            base_spec_sha256,
        } => match &mut app.screen {
            crate::app::root::Screen::Connected(project) => {
                match crate::app::root::spec_reconciliation::apply_approved_delta(
                    project,
                    &ticket,
                    expected_revision,
                    &proposal_sha256,
                    &base_spec_sha256,
                ) {
                    Ok(()) => app.toasts.success(
                        "Applied the approved specification changes; reconciliation will verify the updated specification.",
                    ),
                    Err(error) => app.toasts.warning(error),
                }
            }
            crate::app::root::Screen::Welcome => app
                .toasts
                .warning("Connect a project before approving specification changes."),
        },
        TaskDetailCommand::RunWithoutSharedCoordination { ticket } => {
            app.start_without_shared_coordination(ticket)
        }
        TaskDetailCommand::AuthorizeDependency {
            ticket,
            request_id,
            scope,
        } => app.authorize_dependency_request(&ticket, &request_id, scope),
        TaskDetailCommand::DenyDependency { ticket, request_id } => {
            app.deny_dependency_request(&ticket, &request_id)
        }
        TaskDetailCommand::SetExecutionTarget { ticket, preference } => {
            let task_key = match &app.screen {
                crate::app::root::Screen::Connected(project) => {
                    super::super::super::task_target::key(project, &ticket)
                }
                crate::app::root::Screen::Welcome => ticket.clone(),
            };
            match crate::persistence::execution_environment_settings::save_task_target(
                &task_key, preference,
            ) {
                Ok(()) => app
                    .toasts
                    .success("Saved task execution target preference"),
                Err(error) => app
                    .toasts
                    .warning(format!("Could not save task execution target: {error}")),
            }
        }
        TaskDetailCommand::SetTaskCapabilities {
            ticket,
            requirements,
        } => {
            let task_key = match &app.screen {
                crate::app::root::Screen::Connected(project) => {
                    super::super::super::task_target::key(project, &ticket)
                }
                crate::app::root::Screen::Welcome => ticket.clone(),
            };
            match crate::persistence::execution_environment_settings::save_task_capabilities(
                &task_key,
                requirements,
            ) {
                Ok(()) => app.toasts.success("Saved task target capability requirements"),
                Err(error) => app.toasts.warning(format!(
                    "Could not save task target capability requirements: {error}"
                )),
            }
        }
        TaskDetailCommand::SetTaskAllowedTargets { ticket, targets } => {
            let task_key = match &app.screen {
                crate::app::root::Screen::Connected(project) => {
                    super::super::super::task_target::key(project, &ticket)
                }
                crate::app::root::Screen::Welcome => ticket.clone(),
            };
            match crate::persistence::execution_environment_settings::save_task_allowed_targets(
                &task_key, targets,
            ) {
                Ok(()) => app.toasts.success("Saved task target constraints"),
                Err(error) => app
                    .toasts
                    .warning(format!("Could not save task target constraints: {error}")),
            }
        }
        TaskDetailCommand::GrantRemotePrivateFiles {
            ticket,
            worker_id,
            paths,
        } => app.record_remote_private_file_grant(&ticket, &worker_id, &paths),
        TaskDetailCommand::RevokeRemotePrivateFileGrant { ticket, grant_id } => {
            app.revoke_remote_private_file_grant(&ticket, &grant_id)
        }
        TaskDetailCommand::DiscoverRuntimeTargets { ticket } => {
            app.discover_runtime_targets(&ticket)
        }
        TaskDetailCommand::SelectRuntimeTarget {
            ticket,
            target_id,
            expected_manifest_revision,
        } => app.select_runtime_target(&ticket, &target_id, expected_manifest_revision),
        TaskDetailCommand::ApproveRuntimeRecipe {
            ticket,
            target_id,
            operation,
            approval_proof,
            executable_id,
            argv_lines,
            artifact_paths,
            readiness_port,
            readiness_path,
            expected_manifest_revision,
        } => app.approve_runtime_recipe(
            &ticket,
            &target_id,
            operation,
            approval_proof,
            &executable_id,
            &argv_lines,
            &artifact_paths,
            &readiness_port,
            &readiness_path,
            expected_manifest_revision,
        ),
        TaskDetailCommand::AddRuntimeTarget {
            ticket,
            app_surface,
            project_relative_root,
            artifact_paths,
            build,
            test,
            start,
            required_dependencies,
            approval_proof,
            readiness_port,
            readiness_path,
            expected_manifest_revision,
        } => app.add_runtime_target(
            &ticket,
            app_surface,
            &project_relative_root,
            &artifact_paths,
            &build,
            &test,
            &start,
            &required_dependencies,
            approval_proof,
            &readiness_port,
            &readiness_path,
            expected_manifest_revision,
        ),
    }
}
