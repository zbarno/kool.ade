# Specification reconciliation records

The lifecycle store keeps a per-feature current specification revision at
`state/spec_revisions/{featureUid}.json`, immutable revision snapshots and
deltas at `state/spec_revision_history/`, and event keyed work at
`state/spec_sync_queue/`. Lifecycle event and queue writes share a transaction.
Manual refresh requests use a stable caller-provided request key, so retrying a
request does not create a second queue row.

Each revision records the rendered specification hash, source feature revision,
origin events, decision and approval references, approved target hash, reviewed
implementation head, reconciliation outcome, and timestamp. Delta records
retain old target, proposed target, observed current behavior, classification,
evidence, affected tasks, user reapproval requirement, and follow-up tasks.
Writes validate referenced events, tasks, evidence, approvals, plan requirements,
rendered content, and passing code-review identity. Current revision and
immutable history update atomically under record revision checks. A replay of
the same draft is idempotent; a competing stale writer is rejected. Only a
synchronized revision or one with outstanding nonblocking findings completes
its origin queue entries.

## Current application behavior

| Event or operation | Durable queue behavior | Reconciliation behavior |
| --- | --- | --- |
| Feature, decision, plan review, task graph, task/review/repair/QA, PR lifecycle event | Queue row is written atomically with the lifecycle record. | Task/review/repair/QA triggers are consumed by the task-scoped runner after completed QA. A PR trigger on a completed task schedules a bounded follow-up pass using its reviewed head and persisted provider PR state as trace evidence. |
| Manual refresh API and linked Documentation Refresh reply | Idempotent pending row keyed by feature and message ID; it stores only the conversation message ID while the body stays in user-local storage. | The feature-scoped consumer resolves the exact user message from local conversation history and runs against the exact persisted, user-confirmed, reviewed plan. Missing or oversized message bodies fail closed. It may record documentation findings or proposed future-target changes, but has no task QA/review evidence and rejects current-implementation claims. |
| Approved plan and task generation | Reviewed-plan and task-batch transitions create a revision from the current rendered product specification with their exact lifecycle event IDs. Missing product documents block the transition before lifecycle/task writes. | These are bookkeeping revisions; they do not claim implementation exists. |
| Task passes QA and enters specification reconciliation | A task-scoped trigger is recorded. | Existing typed SpecReconciler runner records a feature revision; proposals remain blocked for user review. |
| User approves a validated delta | Approval receipt and product documents persist; a new history revision records the approved target. | Task reconciliation retries against the updated specification. |
| User adopts or declines a feature-only proposal | The feature Specification view exposes persisted findings, evidence refs, and approval-required classification. The transaction stores an immutable user authorization bound to the source sequence, requirement/event, any present old and proposed text hashes, result product hash, and decision, alongside revised documents and decision history. | Only a matching persisted authorization advances the approved target hash. Mutable decision fields alone cannot authorize adoption. Decline records the decision without changing approved target or current implementation. |
| PR authorization | Authorization is blocked while a pending feature-level trigger or included-task trigger remains unresolved. | It is a durable queue gate, not a merge reconciliation consumer. |
| PR open/close/merge | Provider refresh updates the durable PR preparation record and creates a queue row. | A merged SHA is promoted to a separate current-implementation projection only after the exact base, PR-head, and merge objects resolve locally; the base is an ancestor of the head and merge; and the merge is in the mapped repository's checked-out history. Merge commits require PR-head ancestry; squash commits require identical stable patch IDs from base to PR head and base to merge. The immutable receipt binds task, feature, PR, repository identity, checkout head, and specification revision/hash. Provider metadata alone is not proof. |
| Deployment | No authoritative deployment adapter/event exists in this repository. | Deployment hash remains “Not recorded”; do not infer it from merge or PR state. A deployment provider event adapter and signed/validated source identity are prerequisites. |

Current implementation, approved target, and deployed state must not be inferred
from one another. Task Details, the feature Specification view, and the product
Specification view show the
current rendered product-specification hash, reviewed implementation head, approved plan/target hash,
latest feature revision/status, separately validated current implementation,
and deployment as “Not recorded.” Product-level reconciliation computes a
revision-checked aggregate from the rendered product hash, each lifecycle-backed
feature's latest receipt, pending triggers, and unresolved requirement findings.
It surfaces aggregate drift, missing baselines, pending triggers, and findings
with requirement-scoped decision, task, test, review/QA/PR evidence references;
the source feature receipts remain the durable history. Findings remain open by
their trigger identity until an explicit decision resolves that same trigger.
Decision links resolve only from answered/declined clarification records whose
origin requirement IDs match, or task review/QA receipts on a task explicitly
linked to that requirement by the approved task decomposition; feature-wide
plan receipts are not fanned out. New task records persist the decomposer's
validated requirement IDs only when they resolve within the exact approved
plan snapshot. Older task records deserialize with an empty link set, so the
UI does not infer requirement links for legacy tasks.
The feature view also shows each task's reviewed
head and provider-reported merge commit where available, separately from the
verified current implementation receipt. No deployed-state projection exists
yet. PR status and provider merge metadata alone are not current implementation
conclusions. When a lifecycle feature already
exists, plan confirmation requires a synchronized baseline and rejects hash
drift or pending triggers; first confirmation records the rendered hash in its
durable receipt. Task generation requires a recorded synchronized baseline
and checks drift and pending triggers before writing. Completing a task into PR
preparation applies the same current-hash and no-pending-trigger gates. PR
authorization also checks pending queue entries.
The feature-only runner does not establish current implementation state. There
is still no authoritative deployment event source; deployment remains “Not
recorded” until a provider adapter supplies a validated event bound to the
workspace and deployed artifact.
