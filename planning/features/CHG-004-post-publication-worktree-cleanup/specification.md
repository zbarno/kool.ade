# CHG-004: Post-Publication Worktree Cleanup

**Status:** Draft
**Directed by:** Operator ruling: stale task worktrees must be reclaimed as a terminal step of the per-task workflow (when the task's code is Done and merged), not as a separate periodic cleanup job.
**Bound:** Task-worktree resource lifecycle only. Queue semantics, locks, publication mechanics, PR polling, and reconciliation are untouched.

## Intent

Today, every completed task leaves its persistent worktree (and the per-publication integration worktree) behind in the worktrees root, accumulating one pair of directories plus local branches per finished task. By the time of the operator's request, 21 stale worktrees had accrued in this repository's own development (later partly cleared by a manual pass). The operator's directive is to fold reclamation into the workflow: when a task reaches its terminal done state (confirmed auto-publication, or a polled PR merge mapped to completion), the workflow removes that task's worktree and integration worktree and deletes their directories. No stand-alone cleanup job is owed.

## Current Behavior

- Each task gets a stable worktree (e.g., `<root>/.packet-worktrees/<repo>/<NNN-story-slug-<hash>>` on a `packet/<story>` branch) and each auto-publication transits a separate integration worktree (`packet/integration/<feature>/<hash>` branch); see SPECIFICATION.md, "Ticket implementation, recovery, and Auto mode," and product module 07 (`Implementation` records worktree metadata under Git-private metadata).
- Nothing removes completed worktrees. Code-grounded read-only audit: `src/core/implementation.rs`, `src/core/implementation_queue.rs`, and `src/core/reconciliation.rs` contain no worktree removal/deletion path — only cleanliness guards (e.g., "Worktree is not clean after verification and commit; resume to review") and an implementation prompt that forbids the agent from cleaning or resetting anything.
- The asymmetry is explicit in the docs: terminal failure keeps "its worktree and diagnosis available" (README.md, "Implement a ticket"); success has no symmetric reclamation clause.
- Observed consequence: at the time of the operator ruling, master's worktree list held 21 extra worktrees for already-completed tasks; the operator states a manual cleanup pass is required after every merge/PR.

## Desired Behavior

- On transition to terminal done, the workflow gains a terminal cleanup step for that task only: remove the task worktree and its integration worktree (Git removes the directories and stale registrations), and delete the local branches Packet created for that task/publication only where Git confirms them merged.
- Safety rails: a task that is not terminal-done — in flight, waiting to merge, in review, needs attention, paused by cancellation, or terminal-failed — never loses its worktree; cleanup never force-removes a dirty or divergent worktree and never touches foreign checkouts.
- PR-mode parity: cleanup fires when polling confirms MERGED (completion). CLOSED-unmerged (attention) and REOPENED (back to review) keep the worktree, so reopening still resumes with its history.
- Failure posture: a cleanup failure is a diagnostic. It leaves the worktree in place, never reverses or delays a confirmed publication, and never blocks queue advancement.

## Scope

Adds reclaim-on-done to the ticket workflow. Documents updated: SPECIFICATION.md ticket section, README "Implement a ticket," and product modules 05, 07, and 08 at reconciliation. Out of scope: general Git garbage collection (reflogs, caches, object databases), configurable retention policies, cleaning the originally connected checkout, worktrees owned by other tools or users, and any change to D-14 seat routing, locks, or publication serialization.

## Affected Product Areas

- Module 05 (functional requirements): ticket lifecycle gains terminal cleanup as a published-state effect.
- Module 07 (data model): `Implementation` worktree/branch metadata gains a reclaimed terminal marker.
- Module 08 (architecture): `core` owns the worktree lifecycle step; `harness` stays worktree-unaware.
- SPECIFICATION.md "Ticket implementation, recovery, and Auto mode" and README "Implement a ticket" already carry the added cleanup step wording (written alongside this delta).

## Requirements

- R1: Auto mode — after, and only after, confirmed publication, remove the task worktree and its integration worktree, including their directories.
- R2: PR mode — after polling confirms MERGED and the task maps to completion, remove the task worktree (and its integration worktree, where one existed for the publication).
- R3: Delete Packet-created local task/integration branches only under Git's merged check; otherwise keep them and record why.
- R4: Non-terminal-done tasks and any dirty or divergent worktree keep worktree and diagnosis; cleanup is skipped, never forced.
- R5: Cleanup failure is logged diagnostically; it does not roll back, reverse, or delay confirmed publication, nor block queue advancement.
- R6: Documentation (SPECIFICATION.md, README, modules 05/07/08) reflects the step; no new configuration knob in v1 — cleanup on done is unconditional and automatic.

## Decisions and Assumptions

- Cleanup is per-task and immediate at the done transition, rejecting the alternative batch/GC-job design per the operator ruling.
- Assumes Packet exclusively owns the worktrees-root subtree for the connected repository; only the task's own two worktrees are candidates for removal, nothing discovered incidentally.
- Branch deletion delegates to Git's built-in merged check (`-d` semantics); Packet does not recompute ancestry itself.

## Acceptance Criteria

- AC1: After confirmed auto-publication, the task worktree and integration worktree are gone (registration and directory), `git worktree list` shrinks accordingly, and the board row still reads Done.
- AC2: Equivalent result when a PR polls merged; a PR that remains OPEN or closes unmerged leaves its worktree intact across repeated polls.
- AC3: Cancelling, pausing, or terminally failing a mid-flight task, and later Resuming, still presents its worktree, diagnosis, and prior diffs.
- AC4: With a simulated dirty worktree or missing directory at done time, cleanup is skipped, a diagnostic is recorded, and publication plus queue advancement are unaffected.
- AC5: NFR-8 regression evidence: tests cover the happy paths (AC1–AC2), dirty-skip (AC4), and branch-kept-not-merged; full suite green with no new clippy warnings at the pinned toolchain.
