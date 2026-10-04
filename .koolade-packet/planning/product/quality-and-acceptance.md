## Quality and Acceptance

**Definition-of-done gates** — all three must pass before a task is complete or committed (`AGENTS.md`):

1. `cargo +1.98.1 fmt --all --check`
2. `cargo +1.98.1 test --locked --all-targets -- --test-threads=1`
3. `cargo +1.98.1 clippy --locked --all-targets -- -D warnings`

A gate that cannot run or fails leaves the task incomplete, with the result reported.

**Testing practice.** Co-located unit tests throughout `src/` (for example `src/core/validation/tests`, `src/core/turn/tests`, `src/artifacts/*/`), integration tests (`tests/multi_repository_feature.rs`), and egui renderer fixtures that simulate real clicks and typing against the live UI (board tests under `src/app/root.rs`). Agent behavior is validated with deterministic provider fixtures rather than live models; the task-conversation audit cites 249 library tests plus two integration tests passing at its date — a historical indicator, not a target (`docs/task-conversations-progress.md`).

**Acceptance model.**

- Features: numbered observable acceptance criteria in the change specification, each mapped to its smallest evidence (`docs/chg-001-acceptance-evidence.md`).
- Tasks: each story carries observable acceptance criteria, a test plan, and verification commands that run as independent `/bin/sh` invocations with `$KOOLADE_WORKTREE` available; pipelines that hide exit codes are disallowed (`README.md`).
- Evidence is separated from decisions: implementation reports and verification output are Git-ignored under `.koolade-packet/implementation/` (back them up manually when moving a live workspace); the decision-record collection holds only adopted decisions (`docs/artifact-layout.md`).
- Reconciliation compares merged code against the approved feature contract before product truth advances; material mismatches become review or human decision items rather than silent ratification (`src/core/reconciliation.rs`).
- Board attention cards show the newest available activity first, distinguish user actions from external waits, expose unmet task prerequisites, and highlight cards that share an explicit relationship (`src/ui/layout/board/columns/attention.rs`; `src/ui/layout/board/columns/cards/task.rs`; `src/ui/layout/board/relationships.rs`).

**Failure and recovery standards.** Correction attempts exhaust into two root-cause repairs; interrupts are resumable with worktree and partial state preserved; cleanup is idempotent and preserves changed, unverified, locked, or mismatched work; verification-only contracts may finish with verified evidence and zero repository changes (`README.md`; `docs/task-failure-diagnosis.md`).

**Recorded operational risks.**

- Disk capacity: a 2026-09-22 exhausted volume stopped an in-flight task (reported linker errors remain unverified individually); the 1 GiB start guard plus space monitoring mitigate (`docs/task-failure-diagnosis.md`).
- Process safety in GUI tests: a 2026-09-23 name-matching cleanup killed the supervising app; an ownership-scoped pidfd-based helper mitigated it, and the prohibition on machine-wide name matching is a standing rule.
- Single-host trust: coordination locks live in the host's Git metadata; cross-machine concurrency is unsupported (consistent with D-22).

These are risk notes, not open defect claims; suspected live defects are tracked as board triage items with their uncertainty stated.
