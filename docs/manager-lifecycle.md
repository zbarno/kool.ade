# Man.ager lifecycle contract

This document describes the application-owned transition foundation. It does
not claim that the desktop Man.ager, role prompts, task queue, reviewers, QA
drivers, or PR publisher are wired to it yet.

## Feature transitions

| Current phase | Event | Guard | Next phase |
| --- | --- | --- | --- |
| Exploring | QuestionsRaised | Nonempty bounded question list | AwaitingAnswers |
| AwaitingAnswers | AnswersResolved | Expected revision matches | Exploring |
| Exploring or PlanRework | PlanReady | No unanswered questions; valid plan SHA | AwaitingPlanConfirmation |
| AwaitingPlanConfirmation | UserPlanDecision | User receipt names this feature and exact plan SHA | PlanReviewPending or Exploring |
| PlanReviewPending | PlanReview | Independent reviewer receipt names this feature and exact plan SHA | ReadyToDecompose or PlanRework |
| ReadyToDecompose | TaskGraphCreated | Exact approved plan SHA | Executing |
| Any non-complete phase | Cancel | Expected revision matches | Cancelled |

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

## Evidence shape and remaining integration

Gate receipts carry a subject UID and SHA, actor, decision, evidence references,
and distinct reviewer/author run and session IDs. These checks prevent a review
receipt from reusing the author's recorded context. The lifecycle store still
needs to bind those identifiers to actual application-owned execution records
and authenticated user actions before desktop entry points can accept them.

The coordinator, event replay/recovery, task DAG persistence, role prompt
catalog, question and approval cards, independent code review, browser/desktop
QA, specification reconciliation, and separate PR-preparation task are not
connected yet. Until those integrations are implemented, existing task status
must not be advanced by treating agent prose as a lifecycle event.
