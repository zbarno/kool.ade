# CHG-004: Post-Publication Worktree Cleanup

**Status:** Ready — planning complete. The operator signalled implementation intent in this planning conversation; per the CHG-001/CHG-002/CHG-003 precedent the operative release is the in-app **Approve feature for implementation** action registering the CHG-004 entry in the approval ledger (`.planner/workflow.json`). Ready itself confers no implementation authority.
**Directed by:** Operator ruling: stale task worktrees must be reclaimed as a terminal step of the per-task workflow (when the task's code is Done and merged), not as a separate periodic cleanup job.
**Bound:** Task-worktree resource lifecycle only. Queue semantics, locks, publication mechanics, PR polling, and reconciliation are untouched.

## Intent

Today, every completed task leaves its persistent task worktree and per-publication integration worktree (plus local branches) behind in the worktrees root, accumulating one pair of directories plus local branches per finished task. By the time of the operator's request, 21 stale worktrees had accrued in this repository's own development (later partly cleared by a manual pass). The operator's directive is to fold reclamation into the workflow: when a task reaches its terminal done state (confirmed auto-publication, or a polled PR merge mapped to completion), the workflow removes that task's worktree and integration worktree and deletes their directories. No stand-alone cleanup job is owed.

## Current Behavior

- Each task gets a stable worktree at `<connected-checkout parent>/.packet-worktrees/<project-slug>/<key>` on branch `packet/<key>`; identity persists before creation so crashed runs resume (`src/core/implementation.rs` record construction, `worktree add`, resume guards).
- Auto publication transits a per-task integration worktree: under the per-repository publication lock the pipeline fetches the latest default-branch tip and provisions a **sibling** worktree `<key>-integration-<base[:12]>` on branch `packet/integration/<key>/<base[:12]>` (reused across retries at the same base; a re-fetch at a newer base creates a fresh sibling, leaving stale per-base siblings behind), squash-merges the verified task head there, re-verifies, and publishes one atomic fast-forward commit (same file, auto-publish region; SPECIFICATION.md "Ticket implementation, recovery, and Auto mode"; product module 07 records worktree metadata under Git-private metadata).
- Nothing removes completed worktrees or branches. Code-grounded read-only audit: `src/core/implementation.rs`, `src/core/implementation_queue.rs`, and `src/core/reconciliation.rs` contain no worktree-removal or branch-deletion path — only cleanliness guards (e.g., "Worktree is not clean after verification and commit; resume to review") and an implementation prompt that forbids the agent from cleaning or resetting anything.
- The asymmetry was explicit in the docs: terminal failure keeps "its worktree and diagnosis available" (README.md, "Implement a ticket"); success had no symmetric reclamation clause. (Top-level SPECIFICATION.md and README.md now carry the added cleanup-step wording, written alongside this delta — observation, not implementation.)
- Observed consequence: at the time of the operator ruling, master's worktree list held 21 extra worktrees for already-completed tasks; the operator states a manual cleanup pass is required after every merge/PR.

## Desired Behavior

- On transition to terminal done, the workflow gains a terminal cleanup step for that task only. Candidate resolution is record- and naming-scoped — nothing is ever discovered incidentally: the task worktree, all of that task's integration siblings (`<key>-integration-<base[:12]>`, covering publication-retry residues), and the Packet-created branches `packet/<key>` and `packet/integration/<key>/<base[:12]>`.
- Removal: `git worktree remove` per candidate (directories and stale registrations go with it), executed only where the worktree is clean (empty `git status --porcelain`) and its HEAD commit remains reachable from a surviving local or remote-tracking ref after branch handling; Git's own refusal is the authoritative guard — no `--force`, no manual fallback. A candidate whose directory is already absent degenerates to registration pruning plus diagnostic, not failure.
- Branches: deleted only under Git's merged check (`git branch -d`) — subject to the board-flagged ruling point on the squash-versus-merged-check interaction; refusals keep the branch and record why.
- Safety rails: a task that is not terminal-done — in flight, waiting to merge, in review, needs attention, paused by cancellation, or terminal-failed — never loses its worktree; cleanup never force-removes a dirty or divergent worktree and never touches foreign checkouts (candidates come solely from the task's own records).
- PR-mode parity: cleanup fires when polling confirms MERGED (completion). CLOSED-unmerged (attention) and REOPENED (back to review) keep the worktree, so reopening still resumes with its history. PR mode never provisions an integration worktree (it pushes the task branch directly and opens the PR via `gh` — code-verified), so PR cleanup removes only the task worktree where the candidate set is that small.
- Failure posture: a cleanup failure is a diagnostic. It leaves the worktree in place, never reverses or delays a confirmed publication, and never blocks queue advancement.

## Scope

Adds reclaim-on-done to the ticket workflow. Repository-layer documentation already carries the step wording (SPECIFICATION.md ticket section; README "Implement a ticket", written alongside this delta); product modules 05, 07, and 08 update at reconciliation. Out of scope: general Git garbage collection (reflogs, caches, object databases), remote refs or remote branch deletion (cleanup is local-Git only — network-neutral, NFR-5 posture untouched), configurable retention policies, cleaning the originally connected checkout, worktrees owned by other tools or users, and any change to D-14 seat routing, locks, or publication serialization.

## Affected Product Areas

- Module 05 (functional requirements): ticket lifecycle gains terminal cleanup as a published/completion-state effect.
- Module 07 (data model): the `Implementation` record's worktree/branch metadata gains a reclaimed terminal marker (field shape is an implementation choice; must support "kept, reason" for unmerged branches).
- Module 08 (architecture): `core` owns the worktree lifecycle step; `harness` stays worktree-unaware.
- Top-level SPECIFICATION.md "Ticket implementation, recovery, and Auto mode" and README "Implement a ticket" already carry the added cleanup step wording (written alongside this delta).

## Requirements

- R1: Auto mode — after, and only after, confirmed publication, remove the task's candidate worktrees (task worktree plus all of its integration siblings), including their directories and stale registrations.
- R2: PR mode — after polling confirms MERGED and the task maps to completion, remove the task worktree (and its integration worktrees, where ones existed for the task).
- R3: Delete Packet-created local task/integration branches only under Git's merged check; otherwise keep them and record why (flagged on the board: the confirmed-publication attestation versus strict `-d` ancestry under squash merge — R3 stays ruling-faithful until that settles).
- R4: Non-terminal-done tasks and any dirty or divergent worktree keep worktree and diagnosis; cleanup is skipped, never forced. A candidate whose directory is already absent degenerates to registration pruning plus diagnostic.
- R5: Cleanup failure is logged diagnostically; it does not roll back, reverse, or delay confirmed publication, nor block queue advancement.
- R6: Documentation (SPECIFICATION.md, README, modules 05/07/08) reflects the step; no new configuration knob in v1 — cleanup on done is unconditional and automatic.

## Decisions and Assumptions

- Cleanup is per-task and immediate at the done transition, rejecting the alternative batch/GC-job design per the operator ruling.
- Candidate derivation is scoped to the task's own recorded metadata and documented sibling naming; the assumption that Packet exclusively owns the worktrees-root subtree for the connected repository holds operationally as "act on recorded paths only" — a registration/path mismatch aborts that item with a diagnostic, and nothing discovered incidentally is ever touched (tracked on the board as an assumption).
- Branch deletion delegates to Git's built-in merged check (`-d` semantics); Packet does not recompute ancestry itself. Known interaction: auto-publication delivers the task via a squash commit, so the original task-branch commit is typically *not* an ancestor of the published tip and `git branch -d` usually refuses; the board carries a Review item with a provisional ruling-point (publication attestation) — strict `-d` stands until ruled.
- Code-verified: PR publication never creates an integration worktree; the integration worktree is per-(task, base-commit), recreated when a publication retry's re-fetch advances the base, so stale per-base siblings must join the candidate set.
- Publication is serialized per repository and integration provisioning happens lazily under that lock, so when a task's publication is confirmed the remote tip has moved and no pending task can depend on that task's integration worktrees; defensively, cleanup skips an integration worktree still referenced by another non-done task's record.
- Remote refs are intentionally untouched; cleanup issues local Git commands only.

## Acceptance Criteria

- AC1: After confirmed auto-publication, the task worktree and integration worktree are gone (registration and directory), `git worktree list` shrinks accordingly, and the board row still reads Done.
- AC2: Equivalent result when a PR polls merged; a PR that remains OPEN or closes unmerged leaves its worktree intact across repeated polls.
- AC3: Cancelling, pausing, or terminally failing a mid-flight task, and later Resuming, still presents its worktree, diagnosis, and prior diffs.
- AC4: With a simulated dirty worktree or missing directory at done time, cleanup is skipped, a diagnostic is recorded, and publication plus queue advancement are unaffected.
- AC5: NFR-8 regression evidence: tests cover the happy paths (AC1–AC2), dirty-skip (AC4), and branch-kept-not-merged; full suite green with no new clippy warnings at the pinned toolchain.
