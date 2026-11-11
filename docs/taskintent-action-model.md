# TaskIntent-first action model: plan confirmation and decision surfaces

Status: implemented (branch `feat/taskintent-action-model`). Companion docs:
[manager-lifecycle.md](./manager-lifecycle.md),
[living-specification-policy.md](./living-specification-policy.md),
[planner-policy.md](./planner-policy.md).

## Intent

Every user-visible actionable step is a **durable child task owned by its
TaskIntent**, and every card presents exactly one state-appropriate primary
action area. No approval, readiness conclusion, or state transition may be
inferred from assistant prose, chat text, or card counts — only from typed
lifecycle evidence (snapshots, readiness assessments, receipts, verdicts).

## The canonical "Review and approve plan" card

Implemented as `WorkKind::PlanConfirmation` in
[`src/core/planning_work/plan_confirmation`](../src/core/planning_work/plan_confirmation.rs).
At most **one** card per active feature (duplicate-collapse keeps a
deterministic winner: current-hash match → freshest evaluation → smallest
key). Ownership: `parent_uid` = TaskIntent root, `task_intent_uid` = the root's
stable UID, same `task_intent_cycle`.

| Card status | Derivation (typed evidence only) | What the person sees / can do |
|---|---|---|
| `Todo` | Initial transient before first pass settles it. | Neutral preparing copy. |
| `NeedsAttention` | Plan not ready (assessment missing/outstanding) **or** plan review failed. | Concrete blocker list in plain language; clarify/refine through the card's own conversation. **Never** an approve button. |
| `InReview` | Ready snapshot present and undecided, **or** confirmed but the presented snapshot drifted, **or** re-approval required after changes. | Large **Approve plan & continue** + **Request changes**, the exact immutable snapshot preview, technical provenance behind a collapsible. |
| `InProgress` | Approved; independent plan review running. | Review-running status only. |
| `Done` | Approved **and** review passed. | Outcome; audit trail on the card. |

Invariants:

1. **Idempotent.** A second pass with identical evidence produces no writes,
   no revision bumps, and no repeated announcements (unit + integration tests).
2. **Fail-closed.** Unparseable documents yield a `NeedsAttention` card with a
   concrete repair hint and **no** automatic planner turns.
3. **Prompt-once.** A readiness-evaluation turn is requested at most once per
   spec revision (`evaluation_prompted_for` stamp, cleared when promotable
   stops being true).
4. **Cancellation wins.** Cards whose planning ID is in `cancelled_work` are
   excluded from mutation forever (resurrection prevented, tested).
5. **Binding.** The card stores the exact `snapshot_content_hash` /
   `spec_revision`; approval commands carry the candidate hash, so a revised
   spec automatically moves the same card back to `InReview` instead of
   silently accepting a stale approval.

### Evaluation turns

When a plan is blocked for reasons the Planner can advance (everything except
malformed documents), the sync seam (`src/app/root/plan_confirmation_sync.rs`)
reports `ReadinessEvaluationRequest`s. The periodic sweep
(`src/app/root/tick/activity.rs`) starts at most one
`TurnPurpose::ReadinessEvaluation` turn bound to the card key. The adoption
hook announces instantly but defers launch to the sweep to avoid duplicate
turns.

## Action-mapping inventory

| Human decision | Where it now lives | Authority (typed, never chat) |
|---|---|---|
| Answer a planning question | Question/decision child card under the TaskIntent | Decision recorded via task reply channels; `reconcile_answered_child` advances the parent. |
| Choose among plan alternatives | Plan comparison surface **plus** the canonical card's decision area (both fed by the same `feature_approval` state) | `ApplicationCommand::ChooseFeaturePlan` / `DiscardFeaturePlans`. |
| Approve the plan | Canonical card, `InReview` only | `ApplicationCommand::ApproveFeatureForBoard` + typed approval receipt bound to the snapshot hash. |
| Request changes | Canonical card, `InReview` only | `ApplicationCommand::RequestFeatureChanges { id, candidate_hash }`. |
| Dispose of review/QA feedback | Disposition child cards (pre-existing) | Existing typed disposition commands/records. |
| Grant setup/computer-use permissions | Consent screens (pre-existing) | Explicit typed consent; **intentionally no "Other"** — a free answer must never equal binary consent. |
| Approve/publish PR | Publication card (pre-existing) | Pre-existing typed publication gate. |

Legacy approval UI: feature root cards now show a pointer to the canonical
card instead of duplicating the collapsed "Plan confirmation" section when a
`PlanConfirmation` child exists; the legacy painter only remains for
legacy features without a TaskIntent anchor.

## "Other (write my own answer)"

Added to multi-choice surfaces:

- Planning decision cards (`…/planning/details/decision.rs`)
- Task-detail hero chooser (`…/task_details/action.rs`)
- Change-decision panel (`…/task_details/action/choice.rs`)
- Plan-comparison escape on the canonical card

Routing semantics:

- Custom text travels the **same typed reply channel** as selecting a listed
  option (task-ticket reply), so it is a real decision input, not ambient
  chat. Exact text is preserved in the card conversation.
- It is **never coerced** into an option ID/index; downstream consumers treat
  it as the user's own answer to the decision.
- Binary security/consent confirmations deliberately have no `Other`
  shortcut: declining is always available through the existing refuse path.
- Replies on the plan card are explicitly labelled in the UI as not being an
  approval.

## Verification

Four quality gates (fmt, nextest all-targets, doc tests, clippy `-D
warnings`) plus new automated coverage:

- Core unit suite `planning_work::plan_confirmation` (13 tests): idempotence,
  duplicate collapse, exclusion, multi-feature independence, prompt-once,
  status protection.
- App integration suite `app::root::plan_confirmation_sync::tests` (5
  tests): ready → `InReview` card that survives a private-store reload as a
  strict no-op; unready → `NeedsAttention` + exactly one evaluation request
  per spec revision; malformed documents contained without planner turns;
  operator cancellation protected from resurrection; spec revision moves the
  same card forward.

## Known limitations / follow-ups

- Post-approval receipt drift is enforced by the typed receipt/hash binding
  and the card's `InReview` re-decision path; the drift-after-receipt scenario
  is exercised by the unit reducer tests, while end-to-end receipt replay is
  left to the existing feature-approval flow.
- Plan-comparison free text posts to the card conversation; a future
  "propose my own third plan" candidate generator is a separate feature.
- `Other` on surfaces whose listed options encode authorizations relies on
  the manager's typed decision recording discipline; a regression test suite
  for that downstream behavior is tracked separately.
