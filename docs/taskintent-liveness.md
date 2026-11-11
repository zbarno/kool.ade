# TaskIntent forward progress and recovery

Status: architecture audit for issue #581. This document describes the
current implementation on `main` and separates implemented component behavior
from the cross-lifecycle guarantees that remain work in progress. It does not
claim that every nonterminal TaskIntent is currently guaranteed to advance.

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

The following are not established by the component mechanisms above and must
not be inferred from a green unit test of an individual reducer:

1. There is no cross-lifecycle sweep that visits every current nonterminal
   TaskIntent cycle and proves it has a live lease, an eligible owned/dispatchable
   operation, or one canonical blocker with an executable resolution path.
2. Manager/activity events do not yet share a durable `started → completed /
   failed / timed_out / retried` outcome and checkpoint contract. Existing
   TaskIntent history records committed state changes and gate evidence; it is
   not a complete Manager event cursor or dropped/out-of-order event ledger.
3. Harness preflight requests refresh when selected capability evidence is
   stale, and that attempt returns a pre-dispatch error. Other invalid or
   unsupported route conditions construct an unavailable harness. The code
   does not itself prove that the exact queued operation resumes after a
   successful refresh; it must continue to fail closed and preserve route
   selection while this is addressed.
4. A plan-confirmation reconciliation error currently returns no effects. The
   safe no-mutation behavior is correct, but this seam alone does not guarantee
   a durable user-visible blocker for every storage or validation failure.
5. Existing restart recovery is stage-specific. It does not yet prove the
   complete issue #581 restart matrix across pending answers, active workers,
   cancelled tasks, stale snapshots, and changed Git baselines.

These are implementation gaps, not invitations to add another scheduler,
state store, or event ledger. Follow-up work should extend the existing
authoritative records and dispatch seams, preserve crash-recovery evidence, and
make retry admission bounded and idempotent.

## Acceptance evidence map

| Issue #581 scenario | Current evidence / status |
|---|---|
| 1. Last answer yields review task or explicit blocker | Implemented through #568; app-level deferred-prompt coverage verifies an announcement-only pass survives a private-store reload and the idle sweep claims and returns one readiness request. |
| 2. Full approved-plan-to-merged lifecycle | Not proven end to end by the current component-level lifecycle integrations. |
| 3. Model narration cannot grant readiness or approval | Existing typed readiness/approval boundaries; retain targeted security regressions as coordinator work lands. |
| 4. Stale feature identity recovery | Exact-UID recovery and blocked identity-conflict paths exist; full crash/replay matrix remains to be established. |
| 5. Stale selected harness resumes exact work | Not proven; preflight refresh returns a pre-dispatch error and no automatic exact-operation resumption is established. |
| 6–7. Manager outcomes, malformed envelopes, event gap replay | Not proven by a durable Manager event outcome/cursor contract. |
| 8. Concurrent completion, restart, approval, and ticks | Individual stores use revision fencing/idempotency; combined race matrix remains to be established. |
| 9. All children Done while a gate is unmet | TaskIntent projection consults authoritative feature/task phases and does not derive success from child counts; retain a focused regression across each required gate. |
| 10. Optional setup does not block ordinary work | Existing setup and routing boundaries are separate; a full lifecycle regression remains to be established. |
| 11. Held versus abandoned OS locks | This audit does not establish a complete lock-liveness and safe-cleanup matrix. |
| 12. Stranded task graph converges | No cross-lifecycle liveness checker currently proves bounded convergence. |
| 13. Restart with all specified stale/active inputs | Stage-specific restart coverage exists; the combined scenario remains to be established. |
| 14. Authorization, provider, QA, and publication safeguards | Existing subsystem regressions remain authoritative; a focused result-classifier regression preserves Docker health-disconnect uncertainty, and the existing uncertain-verification consumer regression proves it blocks without dispatching a correction. Coordinator changes must preserve these boundaries. |

Issue #581 should remain open until these gaps have implementation and
executable evidence. Manual/end-user validation remains independent of issue
closure, consistent with the issue's closure policy.
