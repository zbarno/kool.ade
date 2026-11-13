# TaskIntent forward progress and recovery

Status: architecture audit and in-progress implementation for issue #581. This
document describes the baseline implementation on `main` and records this
branch separately. The branch has focused implementation and regression
coverage, but several combined lifecycle and restart acceptance scenarios
remain incomplete. Validation results are recorded with the change review so
this document does not become stale as code and test counts change.
This document does not claim that every nonterminal TaskIntent is guaranteed to
advance.

## Authority and transition graph

TaskIntent roots and child planning work live in the private installation
store. Feature and executable-task phases, immutable plan snapshots, gate
receipts, and publication state are owned by their existing lifecycle stores.
The board and TaskIntent projection aggregate these records; neither card
counts nor Manager prose authorizes transitions. Product specifications remain
in the code repository, as described in the [storage boundary](koolade/architecture/storage-boundaries.md).

```text
user goal
  └─ TaskIntent root / current work cycle
       ├─ planning questions and saved answers
       │    └─ guarded answer application → readiness evaluation
       ├─ immutable plan snapshot
       │    └─ explicit user receipt → independent plan review
       ├─ reviewed plan
       │    └─ task generation → executable task records
       └─ implementation task
            └─ claim-fenced worker → independent review
                 └─ runtime QA → specification reconciliation
                      └─ authorized PR preparation/publication
                           └─ provider outcome (opened / merged)
```

The owners are deliberately distributed. `advance_activity` is the app-owned
periodic dispatcher for planning, recovery, review, QA, and specification
reconciliation. It runs while the connected application is ticking; it is not
a single durable coordinator and does not establish that all transitions have
a queued owner. Each stage must continue to validate its own revision, identity,
provider, authorization, and evidence fences.

## Existing progress and recovery mechanisms

| Concern | Current authority | Behavior already present |
|---|---|---|
| Last planning answer | Answer application and `reconcile_answered_child`; plan-confirmation reducer and periodic activity sweep | The parent is refreshed after an applied answer. The #568 plan-confirmation card evaluates readiness from persisted evidence and becomes either a snapshot-bound review task or a concrete blocker. Adoption announces card changes without stamping an unclaimed readiness request; an idle pass claims and returns one prompt. Provider preflight failure and automatic resumption still require separate coverage. |
| Planning work without a live turn | `reconcile_inactive_planning_work` and core `reconcile_inactive` | Orphaned `InProgress` records are returned to a state-specific review/action state; active turn keys, cancellation, and specialized workers are excluded. |
| Saved-answer failure | Manager operational recovery | Stable recovery identity includes the source task and answer; exact feature identity is checked; native investigation and retries have cooldowns and attempt limits; unsafe identity cases remain actionable blockers. |
| Plan / code review arbitration | Existing review lifecycle and periodic activity dispatch | Where stage-specific recovery exists, interrupted claims are reconciled and returned to bounded retry or visible attention states. Retry coverage is not uniform across review and QA paths. |
| TaskIntent projection | `core::task_intent::TaskIntent` | The aggregate derives lifecycle from root/child statuses, feature/task phases, and typed delivery evidence. It does not treat all-Done child counts as proof of passed gates. |
| Plan-confirmation persistence | `plan_confirmation_sync` and private planning-work store | Reconciliation is idempotent, protects busy/cancelled work, and retries a failed save through a dirty flag. |

The action-card and plan-readiness details are in the
[TaskIntent action model](taskintent-action-model.md). The authoritative
feature/task phase transitions and gate receipts are in the
[Manager lifecycle contract](manager-lifecycle.md).

## Gaps against the no-silent-stall invariant

This branch adds durable Manager run outcomes/checkpoints, exact selected-route
resume after capability refresh, and a periodic TaskIntent liveness sweep. The
sweep creates a cycle-scoped recovery task when it finds no active turn, held
task lease, task-scoped open decision, or recognized lifecycle owner. Its
failure and restart matrix still needs executable validation; it is not yet
evidence that every graph state converges.

The following are not established by the component mechanisms above and must
not be inferred from a green unit test of an individual reducer:

1. The cross-lifecycle sweep now has a persisted recovery-dispatch test across
   restart, Manager event sequence gaps have restart-time detection coverage,
   and ready/unready plan-card save failure now has app-seam fault injection
   for visible retry and successful persistence after store recovery; unready
   evaluation is emitted only after its prompt claim is saved. Duplicate-output
   races and the combined recovery matrix still need application-level fault
   injection.
2. Exact route-preserving resume after capability refresh is implemented in
   this branch. An app-store restart regression now reloads the durable queue,
   verifies fresh proof admits the original ticket, and persists exactly one
   resumed attempt. Real provider verification and worker execution through a
   full application restart remain to be established.
3. A plan-confirmation reconciliation error currently returns no effects. The
   safe no-mutation behavior is correct, but this seam alone does not guarantee
   a durable user-visible blocker for every storage or validation failure.
4. Existing restart recovery is stage-specific. It does not yet prove the
   complete issue #581 restart matrix across pending answers, active workers,
   cancelled tasks, stale snapshots, and changed Git baselines.

These are implementation gaps, not invitations to add another scheduler,
state store, or event ledger. Follow-up work should extend the existing
authoritative records and dispatch seams, preserve crash-recovery evidence, and
make retry admission bounded and idempotent.

## Before / after inventory

| Area | Before this branch | Current branch evidence |
|---|---|---|
| Manager supervision | Lifecycle work could depend on an in-memory Manager result; saved activity lacked a complete run checkpoint/outcome sequence for restart reconciliation. | Manager checkpoints and typed outcomes persist, sequence gaps remain visible, and timeout plus subprocess-kill tests verify bounded replay of the same event batch across app restart. Combined event-order and lifecycle replay coverage remains. |
| Selected provider proof | A stale selected route could return a pre-dispatch blocker without scheduling the queued operation after proof became fresh. | The queue rechecks the persisted provider/role/model binding after restart and admits the original ticket once when proof is current. Full provider execution after application restart remains. |
| TaskIntent liveness | No cross-lifecycle sweep created an owned recovery action for a stranded cycle. | A periodic sweep creates and dispatches one cycle-scoped recovery child, preserves an existing actionable child, bounds restart retries, and reports unreadable task-lease state instead of treating it as a live owner. It now derives task phases from authoritative persisted PR preparation status and defers recovery if that publication record is unreadable or mismatched. Combined failure and graph convergence coverage remain. |
| Authorization and identity | Readiness, review, feature identity, and publication gates already had typed receipts and revision/UID fences. | Those authorities remain in place; the branch does not make narration or child counts authoritative. The combined race and restart matrices remain. |

The original local artifacts cited by issue #581 predate some of the private
state boundaries introduced by #484. They are useful regression scenarios but
do not establish defects in obsolete `.packet` planning stores or current
storage. A zero-byte lock file likewise does not establish a held OS lock; the
new lock tests verify ownership through the operating system.

## Acceptance evidence map

| Issue #581 scenario | Current evidence / status |
|---|---|
| 1. Last answer yields review task or explicit blocker | Implemented through #568; app-level deferred-prompt coverage verifies an announcement-only pass survives a private-store reload and the idle sweep claims and returns one readiness request. A new app-path test persists an applied answer, runs inactive-work reconciliation and plan sync, and verifies one snapshot-bound review task when ready or one specific missing-evidence action when blocked. Ready and unready save-failure cases verify the error is announced, dirty state retries after store recovery, one canonical action survives reload, and an unready evaluation is dispatched only after its prompt claim is durable. |
| 2. Full approved-plan-to-merged lifecycle | A deterministic persisted lifecycle test now traverses answered planning questions, user plan confirmation, independent plan review, task records, implementation/code review, failed and passing QA with repair, spec reconciliation, explicit PR authorization, opened/merged provider state, and the shared board TaskIntent projection. It verifies Preparing PR → PR open → Merged after store reload. A separate production app-tick test dispatches an approved task through the durable Auto queue, isolated clone, local synthetic worker, verifier, and independent-review handoff; it confirms the projection reaches QA only after the review receipt. Real provider/worker execution across the full workflow and automatic cross-stage scheduling remain outside this acceptance path. |
| 3. Model narration cannot grant readiness or approval | The persisted lifecycle acceptance test deliberately supplies a stale `Verified` task projection while the PR authorization record is pending; the shared TaskIntent projection stays at Preparing PR until the typed user receipt and provider outcome are persisted. |
| 4. Stale feature identity recovery | An app-restart regression reloads one failed saved answer and a verified private feature snapshot, restores only its exact UID through the native Manager worker, and verifies the answer remains once with one stable recovery task. A second app-restart regression injects same-display, different-UID replacements with Draft, Ready, Implemented, and Abandoned status; each preserves the replacement UID/status and the saved answer, produces one canonical `NeedsAttention` recovery, and does not retry-loop or revive terminal features. A malformed historical feature snapshot also remains preserved while native recovery creates one stable actionable blocker. The welcome reconnect path now independently proves a malformed current draft blocks loading without rewriting the bytes. |
| 5. Stale selected harness resumes exact work | An app-store restart regression reloads a blocked ticket, applies fresh proof for its unchanged route, and runs the production Auto queue through ImplementationController; the synthetic worker receives the original task UID and the queue records one resumed attempt. A second production Auto-queue regression injects failed proof refresh and verifies the durable UserAction blocker keeps that ticket out of dispatch. Provider evidence and worker are synthetic, so live provider re-verification remains unproven; route-level tests separately verify that an unavailable selected provider has no fallback. |
| 6–7. Manager outcomes, malformed envelopes, event gap replay | Supervised timeout and subprocess-kill fault-injection tests prove durable outcomes and replay of the exact source event after app recovery. Malformed-envelope retention has a private-store preservation test. App-level restart regressions cover both a missing sequence and reordered outcomes: each retains the same existing TaskIntent action, blocks replay, and reproduces the blocker after restart; the reordered case also retains the original checkpoint and verifies the root remains in actionable `NeedsAttention`. A combined app-restart regression now pairs that reordered stream with a genuinely merged lifecycle PR record: it preserves the same Manager blocker and checkpoint, replays the feature lifecycle, and creates no liveness recovery child for the merged TaskIntent. |
| 8. Concurrent completion, restart, approval, and ticks | The lifecycle-store acceptance races two approval receipts against the same persisted PR-work revision and verifies exactly one wins. A second store test races spec completion with user authorization, reopens the store, rejects stale completion replay, and preserves one receipt. The headless app test races its production spec-completion handler with typed authorization, persists the implementation record, restarts the app fixture, reloads that record, and runs ordinary activity ticks. It asserts the same exact receipt remains and the parent task projection is `Publishing`; a later forced tick must leave the full run record and revision unchanged. Both stores are then reopened for projection replay; stale completion replay is independently proved by the lifecycle-store race test. A separate headless app regression verifies an active recovery turn prevents a duplicate child or claim. |
| 9. All children Done while a gate is unmet | Focused projection tests keep all-Done planning children from satisfying plan approval, runtime QA, or publication; each state remains non-complete and reports its next action. Rendered Board regressions cover the Done root and planning children with an unconfirmed plan action, a validated persisted pending Runtime QA projection, and a separate validated pending PR authorization projection. Both gate-pending cases assert the exact nonterminal TaskIntent phase and render the actionable next step. These Board tests use persisted snapshots rather than replaying lifecycle events; lifecycle event/replay correctness is covered by separate lifecycle acceptance tests. |
| 10. Optional setup does not block ordinary work | The app-restart Task Details → queue → worker test persists a required KVM setup flow waiting for an OS action in machine-local setup storage, pumps it to confirm it remains waiting, then resumes the exact approved task from the separate project store. It proves the pending setup flow does not block unrelated work dispatch across the machine/project state boundary. A production setup-pump test reloads a valid Verified task record and advances the same flow automatically through completion without a Resume command. The test seeds typed evidence rather than running host verification. |
| 11. Held versus abandoned OS locks | Focused OS-lock tests distinguish a live owner from an abandoned zero-byte lock file: another owner is rejected while held and can acquire after the original handle closes. Execution-history retention tests refuse to prune a task while its OS lease is held and serialize retention with record updates. A production test simultaneously holds the project-state gate and PlanningStore transaction lock and verifies their files remain in distinct private namespaces. Helper-level regressions cover completed-run receipt cleanup and compose the cancellation watcher with cleanup using a synthetic acknowledgement; real provider acknowledgement remains untested. A welcome-path regression persists a TaskIntent in one project, connects a separate project, then reopens the first and verifies the same task UID and identity remain. Project state has no authoritative inactive/obsolete marker, so switching projects does not prove a shell is safe to delete; cleanup remains limited to resources with lifecycle or lease evidence. |
| 12. Stranded task graph converges | A cycle-scoped queued recovery task is dispatched through the scoped Planner, retried once after restart, then becomes `NeedsAttention`; the sweep preserves an existing actionable child instead of adding a duplicate recovery card. A new app-tick regression polls a scoped synthetic recovery failure, persists its specific blocker as `NeedsAttention`, then reloads the store and verifies repeated ticks retain one canonical action. A larger mixed-cycle app-store test now sweeps three unowned roots in `Todo`, `NeedsAttention`, and interrupted `InProgress` states alongside cycles with an open decision, existing in-review action, active Planner owner, and cancellation; repeated sweeps and reload preserve exactly one stable recovery task per stranded cycle and none for owned cycles. Unreadable task-lease state is surfaced, a stale PlanningStore revision rolls back the in-memory card and converges after resync, and a lifecycle-record read error now creates a stable cycle-scoped recovery action with the diagnostic persisted in the private planning-work store. The app test reloads that action and confirms the normal liveness dispatcher claims it in the same activity pass; other tests confirm recovery is deferred while a QA controller is active or a clarification Planner is queued. Liveness now consumes persisted PR preparation statuses, so a Done task whose PR is Merged is terminal for the sweep; unreadable or mismatched PR state leaves recovery deferred with a specific diagnostic. The combined Manager-replay app test also removes the referenced PR record temporarily and confirms ordinary activity reports the read failure while deferring recovery, then restores it before restart replay. Orphaned in-progress recovery is requeued within its attempt limit. Refresh after revision drift preserves and saves a dirty local record when its durable baseline is unchanged, while a same-record conflict preserves the newer saved detail and status through production inactive-work reconciliation. A new combined graph regression injects an unreadable lifecycle collection and a concurrent PlanningStore revision change, verifies the stale recovery write rolls back, then resyncs and converges three stranded roots while preserving active and cancelled cycles over repeated sweeps. Broader graph convergence and combined failure matrices remain incomplete. |
| 13. Restart with all specified stale/active inputs | An expanded headless app-restart regression combines a pending saved answer, a task execution lock held by a surviving worker subprocess, a cancelled sibling, partial clone progress, approved source SHAs, stale plan-confirmation hash, and changed feature/Git baseline in one persisted workspace. After the app fixture reloads its private store, workspace resync refreshes the changed feature; plan sync updates the existing card to the new immutable snapshot. Assertions retain one actionable review card, cancellation, worker fencing, clone contents, and non-complete task state. The production Task Details → queue → ImplementationController path refuses duplicate execution while the worker process owns the OS lock, then resumes that same task after the helper releases it. A separate production `attempt_connect` regression verifies cancellation and saved-answer reconciliation at connection startup. |
| 14. Authorization, provider, QA, and publication safeguards | Existing subsystem regressions remain authoritative; focused result-classifier regressions preserve the exact local Docker health-disconnect as uncertain independent of its error kind, while pre-session retries remain limited to retryable environment/worker-disconnect errors. The existing uncertain-verification consumer regression proves it blocks without dispatching a correction. Coordinator changes must preserve these boundaries. |

Issue #581 should remain open until these gaps have implementation and
executable evidence. Manual/end-user validation remains independent of issue
closure, consistent with the issue's closure policy.
