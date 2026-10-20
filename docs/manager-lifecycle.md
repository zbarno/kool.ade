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

New lifecycle event records use schema version 3. Feature creation records
carry the revision-zero goal, feature transitions record the resulting feature
revision, and task-creation records carry the initial task plus the resulting
feature and task revisions. Task transitions record their resulting task
revision. Task completion and PR publication events retain the full resulting
PR-preparation projection. `LifecycleStore::replay_feature` reduces the feature,
task, and PR histories without writing, and checks their cross-record links.
PR scope must include the completed task and every directly owned subtask; each
subtask must have a recorded completion before the parent completion event.
Publication authorization is bound to the task's implementation hash and the
explicit create-PR user action. Only after these checks pass does replay restore
missing snapshots, one revision at a time through revision-fenced writes.
Conflicting or malformed snapshots are reported rather than overwritten. Older
version-1 histories and version-2 PR histories remain readable from their
snapshots but cannot restore projections whose original state was not recorded.

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

The code-review subject records the approved plan and task-spec hashes, frozen
head/base and complete diff hash, linked subtasks, affected paths, validated
test receipt hashes, available build artifacts, runtime-QA receipts, and prior
review receipt IDs. The reviewer receives that manifest with the complete diff
and evidence. Repair records also carry a normalized finding fingerprint so a
fresh review cannot resolve a recurring defect merely by changing its finding
ID. Repairs stop at the task's three-attempt budget and move to Needs Attention
for explicit Manager triage.

The application has integration points for explicit user plan confirmation,
independent plan review, reviewed-plan task generation, claim-fenced
implementation, independent code review, applicable runtime QA, specification
reconciliation, and a distinct PR-preparation work item. Current-head code
review findings are supplied to Man.ager as validated evidence and its repair,
user-decision, or follow-up dispositions are persisted as task-linked board
work. Each disposition record carries the finding ID, exact review-subject
hash, decision status, rationale, owner work ID, and source evidence reference
as structured planning-work metadata, including a recurrence count. The
Manager can request user acceptance for a nonblocking residual risk, but only
an explicit Task Details action records RiskApproved; blocking findings
cannot be risk-approved. A passing code review can carry only explicitly
nonblocking findings; specification reconciliation waits until each current
nonblocking finding is promoted to a follow-up or explicitly risk-approved.
Manager-assigned
blocking repairs are revalidated against the current failing receipt and exact
task head before the existing isolated task worker is restarted. The worker
receives a bounded application-owned repair context and must preserve the
approved specification; a fresh independent review resolves or reopens the
repair disposition. If a repair cannot be started or the fresh review still
finds the defect, the work is moved to Needs Attention for Manager triage
instead of being dispatched in an unbounded loop.
Code-review verdicts enumerate examined files, tested evidence IDs, and
unexamined areas. Findings carry a typed category, severity, blocking scope,
file and line location, rationale, affected requirements, and suggested action;
the app validates those fields against the frozen diff and evidence manifest.
The reviewer evaluation fixture seeds both a pass-through abstraction and an
unrelated change. Its default test verifies the typed oracle and that the
production CodeReviewer prompt receives the complete diff. With Pi configured,
the opt-in live check runs `KOOLADE_RUN_LIVE_CODE_REVIEW_EVAL=1 cargo +1.98.1
nextest run --locked --lib --run-ignored ignored-only
live_reviewer_detects_seeded_overengineering_and_unrelated_changes` and
requires the returned verdict to identify both categories. These integrations
are not yet a single end-to-end lifecycle coordinator across
all application paths; legacy feature and task workflows remain. The transition store writes
each snapshot and event in the same revision-checked planning-store
transaction. Implementation `Done` is separate from PR opening and merging.
PR publication requires its own authorization and records its state
independently.

On reopen, `load_feature`, `load_task`, and `load_pr_preparation` validate and
return the current per-record snapshots. `events_for_feature` validates and
sorts matching event records. `replay_feature` reconstructs the projections
from replayable event histories in a read-only validation pass, compares
existing snapshots, then restores missing snapshots through revision-fenced
writes. Legacy histories without
initial-state or resulting-state data remain readable but cannot reconstruct a
missing snapshot. Same-revision concurrent advances are fenced by the
transaction layer, code changes invalidate downstream review and QA evidence,
and completion atomically creates one idempotently keyed PR work item. A
waiting or blocked step retains its reason and evidence rather than
synthesizing a pass. The application still lacks one end-to-end lifecycle
coordinator across all legacy entry points; the local replay tests validate
store recovery, while interactive browser/desktop behavior is covered by the
separate runtime-QA acceptance path.

## PR publication and recovery

The explicit Task Details action authorizes publication of the completed task.
The application compares the verified publication head with the persisted
independently reviewed head before granting or reusing authorization. A Done
task enters PR preparation directly, preserving its completed implementation
and review/QA/specification evidence. When the fetched destination still equals
the reviewed base, preparation preserves the exact reviewed commit. Destination
drift uses the existing isolated integration controller; a changed candidate
cannot inherit the old approval and requires fresh review, QA and specification
evidence. The destination SHA is checked again before pushing.

The read-only PR Preparer's validated description is written to the body file
consumed by the provider controller, alongside application-derived scope
identifiers, commits and changed files. Reusing its receipt rebuilds that body,
including after a crash between receipt persistence and body creation. The raw
evidence bundle is not uploaded as a PR description.

A publication error records `NeedsAttention` on the existing PR work item with
a bounded, redacted blocker and attempt counter. The implementation remains
Done. Task Details offers **Retry PR publication**; retries retain the original
user receipt, recheck current specification/head gates, and query existing
provider PRs before creating one. A lost creation response is an unknown remote
outcome, not proof of failure. Duplicate retry clicks reuse the same publishing
attempt. Failure and retry events retain their snapshots for replay and record
an explicit no-new-specification-evidence disposition. New requirements or
other pending specification triggers continue to block authorization.

Older PR records without recovery metadata remain readable. This recovery
contract does not yet provide the full publication-generation/lease snapshot
specified by issue #164. The separate Generate PR board task and dry-run
Details flow, automatic integrated-head gate renewal, and verified child-commit
aggregation also remain incomplete; child integration depends on issue #162.
