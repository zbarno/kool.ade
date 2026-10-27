# Output schema index (v1)

The canonical strict Rust validators are in `src/harness/prompt_catalog/schemas.rs` and the setup-assistant contract in `src/harness/api/bootstrap_conversation.rs`; every object uses `deny_unknown_fields` and known enums.

- ManagerOutputV1: `proposed_actions[]` with kind, target, reason, evidence and expected_revision; optional OpportunityProposalV1 list.
- PlannerOutputV1: proposed_questions (question, why_it_matters, options, recommendation, impact_of_no_answer), draft_changes, readiness (ready, blockers, evidence).
- ReviewVerdictV1: subject_hash, pass, coverage, findings, untested. Coverage status is pass/fail/untested; finding severity and kind are closed enums.
- TaskGraphV1: approved_plan_hash and tasks with subtasks, depends_on, acceptance_criteria, tests, source_branch and destination_branch.
- WorkResultV1: strict schema-v2 implementation report with `schemaVersion`, `status`, `blocker_disposition`, `summary`, `acceptance_criteria`, `verification`, `remaining`, and `human_choices`. The application rejects unknown schema versions and then applies the existing ticket, blocker, evidence, and verification checks.
- SpecDeltaV1: requirement ID, old/new, current-truth/future-target/documentation classification, evidence, approval effect and follow-up task IDs.
- PRProposalV1: task UID, base/head refs, commits, files, evidence and description.
- BootstrapConversationResponseV1: strict `schemaVersion: 1`, a closed reply-kind enum rendered by Kool.ad/e from current app-owned task facts, and an optional typed suggestion. Free-form assistant text is rejected. Only `recheck_prerequisites`, `prepare_locked_image` (with one bounded `imageProfile`), and `open_coding_tools` are accepted; the assistant cannot return commands, tool calls, IDs, or arbitrary parameters.

OpportunityDecisionV1 is an application-owned user receipt shape (`proposal_hash`, `choice`); it is deliberately not a field in Manager or Planner output. The model cannot produce the receipt that authorizes planning.
