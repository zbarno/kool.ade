## Current Capabilities

Evidence base: `README.md`, `docs/`, the `src/` module map, and the acceptance audits cited inline. Statements describe observed, tested behavior.

**Planning and living specification**

- Conversational planning turns run against a local Pi CLI agent; strict per-operation response schemas; only validated mutations are applied, atomically, then Git-checkpointed (`src/core/turn.rs`, `src/core/validation.rs`, `src/core/apply.rs`).
- One logical specification: an index plus ordered modules with six required concepts; optional modules are added only for real project needs (product index; `docs/planner-policy.md`).
- Change specifications (e.g. `CHG-001`) carry a typed lifecycle; merged code is inspected before product truth advances (`src/core/reconciliation.rs`; `docs/chg-001-acceptance-evidence.md` AC8).
- Open items (`CLR-nnn`) have independent kind (Question/Ambiguity/Assumption/Ownership), priority, and authority; chat-question selection enforces the D-14 routing law (`src/core/routing.rs`, `src/core/validation.rs`; chg-001 AC6).
- Architectural decision records are created only for durable, consequential choices the user approves; two-plan comparisons carry an advisory recommendation plus explicit operator adoption (`.koolade-packet/state/workflow.json`; `README.md`).

**Board and conversation**

- Five-lane Kanban — To do, In progress, In review, Needs attention, Done — mixing planning items and implementation tasks, with type badges and ten-second-bucket activity graphs globally and per card (`README.md`; `src/ui/planning_board.rs`).
- Needs Attention sorts by the latest known conversation or task activity and labels cards `Waiting on user` or `Blocked`; task cards name unfinished prerequisites. Hovering a card highlights others linked by feature, task batch, parent work, or explicit dependency (`src/ui/layout/board/columns/attention.rs`; `src/ui/layout/board/relationships.rs`).
- The read-only project manager reviews board events and task progress while the workspace is open, independently of Auto Plan. It points to user actions and external waits without changing queue settings or task state (`src/app/manager.rs`; `src/app/manager/prompt.rs`).
- Per-item focused conversations: the inline reply and the expanded history share one draft and one message set; histories never inherit other items' context (`docs/task-conversations-progress.md`; `src/core/task_conversation.rs`).
- Board actions (approvals and answers directly on cards) mutate state atomically without chat (chg-001 AC22).

**Task generation**

- Ordered task stories generated from an approved feature specification; up to six repair attempts per outline and per story; resumable private checkpoints; coverage and dependency validation on completion; regenerated batches use suffixed directories such as `<batch>-02` (`README.md`, From interview to task stories; `src/core/task_generation.rs`).

**Implementation and publication**

- Concurrent worker pool (default 3, configurable 1–8) in isolated per-worker worktrees; Auto Build continues ready tasks while planning and task-generation turns run; one task's review or attention state does not stop unrelated ready work; ready tasks waiting for slots remain queued and are reported as capacity waits; dependencies require completed predecessors (chg-001 AC9).
- Recovery ladder: three normal corrections, then two root-cause repair attempts; harness-loss recovery recovers final messages or per-attempt report files before committing; explicit cancellation semantics; a one-shot automatic resume budget for recognized parked failure classes (`README.md`, Implement a ticket; `src/core/implementation.rs`, `src/core/implementation_queue.rs`).
- Publication serializes per repository: an integration worktree from the latest remote default branch, squash merge, re-ran checks, one atomic commit, fast-forward push; lost pushes recover by checking the saved commit against remote history; post-publication cleanup preserves changed, unverified, locked, or mismatched work (`README.md`).
- Connected pull requests are checked every minute: merged becomes Done, closed raises attention, reopened returns to review (`README.md`).

**Durability, artifacts, migration**

- A single live shared artifact root, `.koolade-packet/`; versioned metadata drives lifecycles (visible status text cannot set them); transactional, journaled writes with interruption recovery (chg-001 AC12); legacy-tree migration prefails conflicts by preserving both copies (`docs/artifact-layout.md`; `src/artifacts/`).
- Maintenance binaries: `task_status` (read-only audit) and `cleanup_completed` (one-off maintenance pass) (`examples/`; `README.md`).

**Host integration**

- The Pi harness runs as a supervised child process with an NDJSON event stream; Bubblewrap sandboxes provide environment scrubbing, hidden credential/home directories, read-only repository mounts, and network isolation; a host-side relay serves only a configured private HTTP OpenAI-compatible provider over a Unix socket with a placeholder key inside the sandbox; public/HTTPS providers fail closed (`src/harness.rs`; `README.md`, Host execution capabilities).

**Explicit non-goals today**

- No semantic/vector retrieval by design; selection is explicit logical module IDs plus stable `F-`/`FR-`/`NFR-`/`D-` references (chg-001 AC17–AC18).
- No Windows/macOS support; no multi-operator collaboration; unowned-lane routing relies on the seated operator inheriting them.
