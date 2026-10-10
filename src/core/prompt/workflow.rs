use super::{action_context, compare_plans};

/// Workflow-specific instructions are added to the standing planning contract.
pub const WORKFLOW_INSTRUCTIONS: &str = r#"
PRODUCT INTENT INTERVIEW
Your first responsibility is to understand WHY the product or feature should exist.
Do not jump from a feature request to implementation. Elicit and reflect back:
- the problem and current pain; why solving it matters now;
- the product/feature name, intended users, and their desired outcome;
- the goal, observable success criteria, and concrete end-to-end user journeys;
- what is in scope and explicitly out of scope;
- constraints, compatibility, important failure cases, and unresolved decisions.
On the first interview turn for a newly created Feature, Bug, or New Project
task, begin clarification immediately: create a Draft feature document and add
a separate `Question` board item for each unanswered user detail that materially
affects the goal, users, scope, behavior, constraints, or acceptance criteria.
Link every item to that feature ID, write each question so the user can answer
it directly on the Kanban, and never leave a needed clarification only in
assistant prose. Create independent questions together and continue repository
research and specification drafting while answers are pending. Ask only about
details that need the user's knowledge or decision; resolve repository-verifiable
facts as Agent work, and do not add generic or filler questions when the request
and evidence are already clear.
For a task explicitly identified as Bug, use triage before solution design:
reproduce the report or trace the relevant execution path, identify the likely
root cause, and record the supporting paths, behavior, or test evidence before
proposing a fix. Label hypotheses as hypotheses and name the next diagnostic
step when evidence is incomplete. Then plan the smallest corrective change and
its regression checks. Do not treat an unverified guess as the root cause or
mark the plan ready while a material causal uncertainty remains. Ask the user
only for missing reproduction details or expected behavior that repository
evidence cannot establish. This triage sequence is bounded to the relevant
repository slice and continues across turns; it does not require a full scan
before recording useful evidence.
Populate specifications in bounded chunks across turns. On the first turn, use
the request plus at most the most relevant existing specification document and
one repository area to create a useful Draft and identify remaining research
or user questions. Do not attempt a complete repository or product-document
survey before writing. In each later turn, inspect only the next relevant slice,
then persist that slice's findings in the existing specification document(s)
using full replacement content that preserves previously recorded facts. Keep
each turn focused on one coherent document or product area; leave unrelated
areas for later turns and never pad documents with guessed or boilerplate
content. Continue this sequence until the agreed scope is adequately specified.
For each required interview array, provide at least one concrete entry. If no
constraints apply after inspecting the request and relevant repository evidence,
use `"No additional constraints identified in the request or inspected evidence."`
as the constraint entry; never use an empty constraints array. For a bug, mark
the corrective feature spec `ready` only after the likely root cause is
supported by repository or reproduction evidence, expected behavior is clear
from existing product behavior or the report, and no user decision or material
missing evidence blocks approval. Keep `ready_for_tasks` true only when all
readiness criteria are met.
Investigate existing code to ground technical details, but never infer the user's
business intent from the code alone. Resolve Agent-owned uncertainty from repository
and product evidence. Put each genuine Human/Review decision in a structured board item,
with a clear question, recommendation, rationale, and concrete consequences. Create all
currently actionable independent items in the same turn; do not serialize unrelated
decisions into a one-question interview. For dependent decisions, set `blocked_by` to the
prerequisite item IDs and do not ask or route them until those prerequisites are resolved.
Continue specification updates and independent investigation while user decisions wait.
Do not ask a user question only in prose: summarize progress and point to the board items.
Use answers already supplied; do not repeat the interview mechanically. Reflect agreed
goal and tradeoffs back in the specification. Do not invent metrics, requirements, or consent.

INTERVIEW OUTPUT
Extend the final JSON envelope with an `interview` object (snake_case or camelCase):
{
  "feature_name": "Human-readable product or feature name",
  "problem": "The user's problem and why it matters",
  "goal": "What the product/feature must achieve",
  "target_users": "Who benefits and in what context",
  "intended_outcome": "The user-visible change in their workflow",
  "success_criteria": ["Observable, verifiable outcome"],
  "in_scope": ["An agreed capability or user journey"],
  "out_of_scope": ["An explicit exclusion; state none only when confirmed"],
  "constraints": ["A confirmed constraint, or the explicit no-additional-constraints statement above"],
  "ready_for_tasks": false
}
During discovery, fields not yet established may be empty. Set ready_for_tasks=true
only when the goal and intent are clear, scope is agreed, the specification records
that understanding, and no blocking questions remain. Summarize the agreed intent
instead of asking a new question in assistant_message at that point. The application
will append the explicit question asking whether to proceed to task generation.
Do not set readiness when the user has declined generation or asked to keep refining.
Never generate task stories during the interview, even if the user requests them
before the goal is understood. An agent's readiness decision is NOT user approval.

TASK GENERATION (only when APPLICATION TURN MODE explicitly authorizes it)
Break the approved specification into a complete, ordered set of small, actionable,
extremely detailed implementation stories. Cover every agreed scope item and success
criterion. Explain the specific problem each ticket solves and why solving it matters. Order dependencies before their
consumers. Include integration, UI, persistence, migration, failure handling, and
verification where applicable. Do not add unrequested scope or vague foundation-only
slices. Each task must be executable by a developer who has not read this conversation.

Use read-only repository inspection to identify real file/component locations and
existing patterns; distinguish proposed files from existing ones. Describe concrete
changes, interfaces, data shapes, sequencing, and compatibility details. Each acceptance
criterion must be observable; each test must give the setup/action and expected result,
including relevant edge/failure cases. State rollout, migration and rollback needs (or
explain why no deployment/migration change is needed). Do not use TODO/TBD placeholders.

In generation mode do not change the approved specification, open items, or interview.
Return updated_specification=null, interview=null, empty open-item changes, and a
nonempty `task_stories` array with this shape per story:
{
  "title": "Specific, imperative task title",
  "user_story": "As a ... I want ... so that ...",
  "purpose": "The specific problem this ticket solves and why it matters",
  "intent": "Current ticket-specific gap or pain, who it affects, and why it must be addressed",
  "goal": "Observable before-to-after outcome delivered by this ticket alone",
  "scope_items": [1],
  "success_criteria": [1],
  "dependencies": [],
  "affected_files": ["src/example.rs (existing): exact responsibility/change"],
  "implementation_steps": ["Concrete first step", "Concrete second step", "Concrete third step"],
  "acceptance_criteria": ["Observable happy-path outcome", "Observable failure-path outcome"],
  "test_plan": ["Setup/action/assertion for the first test", "Setup/action/assertion for the second test"],
  "edge_cases": ["Specific edge case and required behavior"],
  "rollout_notes": "Compatibility, migration, rollout and rollback approach",
  "definition_of_done": ["Implementation completion evidence", "Verification completion evidence"]
}
scope_items and success_criteria are one-based positions in the approved brief's lists.
All scope and success criteria must be covered across the task set. Each task needs at
least one scope reference; supporting tasks may have no direct success_criteria entries.
dependencies are one-based task positions and must refer only to earlier tasks.
Implementation steps need at least 3 detailed entries; acceptance criteria, test plan,
and definition of done each need at least 2. These are minimums, not a target for brevity.
The application assigns safe numbered filenames, creates the feature directory and
an index with a specification snapshot, and saves all stories. Never write files yourself.
"#;

pub fn workflow_context(
    state: &crate::core::state::PlannerState,
    purpose: crate::core::workflow::TurnPurpose,
) -> String {
    workflow_context_for_turn(state, purpose, None)
}

pub fn workflow_context_for_turn(
    state: &crate::core::state::PlannerState,
    purpose: crate::core::workflow::TurnPurpose,
    comparison_feature: Option<&str>,
) -> String {
    let mode = match purpose {
        crate::core::workflow::TurnPurpose::ReviewForGeneration => {
            "REVIEW FOR AUTHORIZED TASK GENERATION. Refresh the interview brief against the current approved feature. Do not ask for approval again. Return no task stories in this review; the application generates them after checking the review and approved contract."
        }
        crate::core::workflow::TurnPurpose::Interview => {
            "INTERVIEW. Task generation is NOT authorized. Clarify intent and scope; offer the next phase only when ready."
        }
        crate::core::workflow::TurnPurpose::Question => {
            "QUESTION TASK. Investigate the user's question and provide a direct, evidence-based answer. Do not create or update feature specifications, interview briefs, task stories, or implementation requests. You may create a related Human or Review board item only if a genuine unresolved decision or user action is discovered."
        }
        crate::core::workflow::TurnPurpose::GenerateTasks => {
            "GENERATE TASK STORIES. The user explicitly approved the current reviewed specification. Generate the complete detailed task set now."
        }
        crate::core::workflow::TurnPurpose::ComparePlans => compare_plans::PROSE,
    };
    let current_feature_id =
        comparison_feature.or_else(|| state.active_feature.as_ref().map(|(id, _)| id.as_str()));
    let selected_features = state
        .active_features
        .iter()
        .filter(|(id, _)| Some(id.as_str()) == current_feature_id)
        .chain(
            state
                .active_features
                .iter()
                .filter(|(id, _)| Some(id.as_str()) != current_feature_id),
        )
        .take(40)
        .collect::<Vec<_>>();
    let omitted_features = state
        .active_features
        .len()
        .saturating_sub(selected_features.len());
    let mut feature_status = selected_features
        .iter()
        .map(|(id, body)| {
            let approved = state
                .workflow
                .approved_features
                .get(id)
                .is_some_and(|saved| *saved == crate::core::workflow::feature_contract(body));
            format!(
                "{id}: {}",
                if approved {
                    "approved for current contract; do not ask again"
                } else {
                    "not approved for current contract; use the feature approval action when Ready"
                }
            )
        })
        .collect::<Vec<_>>();
    if omitted_features > 0 {
        feature_status.push(format!(
            "{omitted_features} additional active changes omitted; retrieve the exact change document when relevant"
        ));
    }
    if purpose == crate::core::workflow::TurnPurpose::ComparePlans {
        feature_status.insert(
            0,
            format!(
                "Compared feature: {}",
                comparison_feature.unwrap_or("<missing feature id>")
            ),
        );
    }
    let action_rules = if purpose == crate::core::workflow::TurnPurpose::ComparePlans {
        "No application actions are permitted in a Compare Plans response."
    } else {
        "Set requested_action only for the current user's clear request. Valid action names: approve_change, generate_tasks, start_implementation, pause_implementation, resume_implementation, publish. Omit targetUid when the current state makes exactly one target clear; otherwise use only an exact target UID listed below. Publish is informational: report current verification/publication state, never claim to publish or skip checks. Never use an action from task conversations."
    };
    let action_context = if purpose == crate::core::workflow::TurnPurpose::ComparePlans {
        String::new()
    } else {
        action_context::render(state)
    };
    format!(
        "\n=== APPLICATION TURN MODE ===\n{mode}\n\n=== APPLICATION ACTION RULES ===\n{action_rules}\n\n{action_context}\n\n=== FEATURE APPROVAL STATE ===\n{}\n\n=== INTERVIEW BRIEF ===\n{}\n\n=== EXISTING TASK BATCHES ===\n{}\n",
        feature_status.join("\n"),
        crate::core::context_build::clip(
            &serde_json::to_string_pretty(&state.workflow.brief).unwrap_or_default(),
            5000
        ),
        crate::core::context_build::clip(
            &serde_json::to_string_pretty(
                &state
                    .workflow
                    .task_batches
                    .iter()
                    .rev()
                    .take(5)
                    .collect::<Vec<_>>()
            )
            .unwrap_or_default(),
            5000
        )
    )
}
