# Man.ager lifecycle contract

`LifecycleStore` is the durable application-owned state machine for the
feature-plan and implementation pipeline. Manager and worker output supplies
proposals and evidence; Rust validates every transition before persisting it
to the planning repository. Some Task Details flows project persisted
lifecycle records, while other screens and legacy planning paths still use
their existing models. This page describes the typed lifecycle domain and
store contract; it does not imply that every application path is integrated.
Freeform specification chat remains separate from per-item work.

## Feature transitions

| Current phase | Event | Guard | Next phase |
| --- | --- | --- | --- |
| Exploring | QuestionsRaised | Nonempty bounded question list | AwaitingAnswers |
| AwaitingAnswers | AnswersResolved | Expected revision matches | Exploring |
| Exploring or PlanRework | PlanReady | No unanswered questions; valid plan SHA | AwaitingPlanConfirmation |
| AwaitingPlanConfirmation | UserPlanDecision | User receipt names this feature and exact plan SHA | PlanReviewPending or Exploring |
| PlanReviewPending | PlanReview | Independent reviewer receipt names this feature and exact plan SHA | ReadyToDecompose or PlanRework |
| ReadyToDecompose | TaskGraphCreated | Exact approved plan SHA | Executing |
| Any non-terminal phase | Cancel | Expected revision matches; recorded as a user action | Cancelled |

New tasks can be recorded only after their feature reaches `Executing`, and the
task must reference the same approved plan hash. Feature and task snapshots are
per-record files in the planning repository's `state/lifecycle_*` collections.
They use the existing planning-store transaction and revision-fencing layer,
while avoiding collisions with canonical workflow records (`state/workflow/`),
task state (`state/tasks/`), and board work records (`state/work/`). Each
transition writes an immutable event record and advances the snapshot atomically.

## Task transitions

| Current phase | Event | Guard | Next phase |
| --- | --- | --- | --- |
| Ready | Start | Expected revision matches | Running |
| Running | ImplementationFinished | Valid code SHA | AwaitingReview |
| AwaitingReview or ReReview | CodeReview | Independent context and exact code SHA | AwaitingRuntimeQa or Repairing |
| Repairing | BeginRepair | Expected revision matches | Running |
| AwaitingRuntimeQa | RuntimeQa | Independent review of exact code SHA, or application evidence for NotApplicable | AwaitingSpecReconciliation or Repairing |
| AwaitingSpecReconciliation | ReconcileSpec | Application receipt covers exact code SHA; PR-preparation UID supplied | Done |

`Done` is rejected unless current-code review, runtime-QA disposition,
specification reconciliation, and PR-preparation linkage are all present.
Updating the implementation SHA clears all downstream receipts. Snapshot and
event writes share one transaction, so a stale revision, duplicate event UID,
or failed write leaves the prior snapshot intact.

## Evidence and application wiring

Gate receipts carry a subject UID and SHA, actor, decision, and evidence. Plan
and code review receipts also bind the versioned reviewer prompt and distinct
author/reviewer run and session identities. The app creates these receipts
from the current prompt execution and validates them against the exact plan,
code head, and review subject before accepting them.

The application has integration points for explicit user plan confirmation,
independent plan review, reviewed-plan task generation, claim-fenced
implementation, independent code review, applicable runtime QA, specification
reconciliation, and a distinct PR-preparation work item. These integrations
are not yet a single end-to-end lifecycle coordinator across all application
paths; legacy feature and task workflows remain. The transition store writes
each snapshot and event in the same revision-checked planning-store
transaction. Implementation `Done` is separate from PR opening and merging.
PR publication requires its own authorization and records its state
independently.

On reopen, `load_feature` and `load_task` validate and return the current
per-record snapshots. `events_for_feature` validates and sorts matching event
records for inspection; it does not replay them or rebuild a missing or
corrupt snapshot. The event history is therefore an audit trail, not a
reconstruction source. In particular, `FeatureCreated` does not contain the
initial feature goal, so the current event records are not sufficient to
reconstruct a feature from scratch. Same-revision concurrent advances are
fenced by the transaction layer, code changes invalidate downstream review
and QA evidence, and completion atomically creates one idempotently keyed PR
work item. A waiting or blocked step retains its reason and evidence rather
than synthesizing a pass. Deterministic lifecycle replay/reconstruction and
restart replay coverage remain open work tracked by issue #157. Local
lifecycle acceptance fixtures validate the implemented store contracts;
interactive browser/desktop behavior is covered by the separate runtime-QA
acceptance path.
