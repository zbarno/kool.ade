use super::RolePayloadV1;

pub(crate) fn validate_payload(payload: &RolePayloadV1) -> Result<(), String> {
    fn text(value: &str, max: usize) -> bool {
        !value.trim().is_empty() && value.len() <= max && !value.contains('\0')
    }
    fn list(values: &[String], count: usize, max_item: usize) -> bool {
        values.len() <= count && values.iter().all(|value| text(value, max_item))
    }

    let valid = match payload {
        RolePayloadV1::Manager {
            board_event_cursor,
            work_graph,
            unresolved_findings,
            latest_review_receipts,
        } => {
            text(board_event_cursor, 128)
                && list(work_graph, 256, 4096)
                && list(unresolved_findings, 256, 4096)
                && list(latest_review_receipts, 256, 512)
        }
        RolePayloadV1::Planner {
            open_questions,
            previous_answers,
            research_evidence,
            requested_scope,
        } => {
            list(open_questions, 256, 4096)
                && list(previous_answers, 256, 4096)
                && list(research_evidence, 256, 1024)
                && text(requested_scope, 16 * 1024)
        }
        RolePayloadV1::PlannerOpportunity {
            open_questions,
            previous_answers,
            research_evidence,
            requested_scope,
            opportunity_origin,
            existing_support_evidence,
            workaround,
            user_value,
            concrete_target_repo,
            ..
        } => {
            list(open_questions, 256, 4096)
                && list(previous_answers, 256, 4096)
                && list(research_evidence, 256, 1024)
                && text(requested_scope, 16 * 1024)
                && opportunity_origin
                    .as_ref()
                    .is_none_or(|value| text(value, 128))
                && list(existing_support_evidence, 256, 1024)
                && workaround.as_ref().is_none_or(|value| text(value, 4096))
                && user_value.as_ref().is_none_or(|value| text(value, 4096))
                && concrete_target_repo
                    .as_ref()
                    .is_none_or(|value| text(value, 128))
                && (opportunity_origin.is_none()
                    || (user_value.is_some() && concrete_target_repo.is_some()))
        }
        RolePayloadV1::PlannerTurn {
            turn_context,
            workflow_instructions,
            operator_persona,
            task_mode_note,
        } => {
            !turn_context.trim().is_empty()
                && turn_context.len() <= 256 * 1024
                && workflow_instructions
                    .as_ref()
                    .is_none_or(|instructions| instructions.len() <= 64 * 1024)
                && operator_persona.len() <= 16 * 1024
                && task_mode_note
                    .as_ref()
                    .is_none_or(|note| note.len() <= 16 * 1024)
        }
        RolePayloadV1::Review {
            subject_revision,
            subject_hash,
            author_run_id,
            reviewer_run_id,
            author_session_id,
            reviewer_session_id,
            requirements_matrix,
            explicit_exclusions,
        } => {
            text(subject_revision, 512)
                && subject_hash.len() == 64
                && subject_hash.bytes().all(|byte| byte.is_ascii_hexdigit())
                && text(author_run_id, 128)
                && text(reviewer_run_id, 128)
                && author_session_id
                    .as_ref()
                    .is_some_and(|value| text(value, 128))
                && reviewer_session_id
                    .as_ref()
                    .is_some_and(|value| text(value, 128))
                && list(requirements_matrix, 512, 4096)
                && list(explicit_exclusions, 256, 4096)
        }
        RolePayloadV1::QaScenarioSynthesis {
            immutable_request_json,
        } => text(immutable_request_json, 64 * 1024),
        RolePayloadV1::Decomposer {
            approved_plan_hash,
            approved_scope,
        } => {
            approved_plan_hash.len() == 64
                && approved_plan_hash
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
                && text(approved_scope, 16 * 1024)
        }
        RolePayloadV1::Worker {
            assigned_task,
            assigned_subtasks,
            immutable_baseline,
            allowed_execution_tools,
        } => {
            text(assigned_task, 16 * 1024)
                && list(assigned_subtasks, 256, 4096)
                && text(immutable_baseline, 512)
                && list(allowed_execution_tools, 128, 128)
        }
        RolePayloadV1::SpecSync {
            approved_requirements,
            verified_implementation_refs,
        } => {
            list(approved_requirements, 512, 4096) && list(verified_implementation_refs, 512, 1024)
        }
        RolePayloadV1::Repair {
            assigned_blocking_findings,
            approved_task_baseline,
            allowed_execution_tools,
        } => {
            list(assigned_blocking_findings, 256, 4096)
                && text(approved_task_baseline, 512)
                && list(allowed_execution_tools, 128, 128)
        }
        RolePayloadV1::Pr {
            reviewed_task_refs,
            approved_subtasks,
            base_ref,
            head_ref,
        } => {
            list(reviewed_task_refs, 256, 1024)
                && list(approved_subtasks, 256, 1024)
                && text(base_ref, 512)
                && text(head_ref, 512)
        }
    };
    if valid {
        Ok(())
    } else {
        Err("role payload contains empty, oversized, or excessive fields".into())
    }
}
