# Repository artifact layout

Reviewed against the checked-out implementation on 2026-09-24.

| Location | Purpose and disposition |
| --- | --- |
| `planning/product/` | Current product authority: index and thirteen modules. Retained where the planner reads and updates them. |
| `planning/features/` | Six feature contracts. CHG-001 and CHG-002 are marked Implemented; CHG-003 is Implementing; CHG-004–006 are Ready. Retained for approval, reconciliation, stable IDs, and historical intent. These labels are planning state, not a fresh certification of implementation completeness. |
| `planning/open-items.md` | Current queue, presently empty. Required serializer input; an empty queue is valid state. |
| `planning/resolved-items.json` | Resolved questions and outcomes. Retained for board history and context. |
| `.planner/config.md` | Current repository stakeholders and identity fallback. Required configuration. |
| `.planner/workflow.json` | Approved feature contracts, task-batch references, and workflow state. Required for resumability; preserved byte-for-byte. |
| `.kool-ade-packet/planning/tasks/` | Approved task stories and frozen contracts. Retained as immutable implementation inputs. |
| `.kool-ade-packet/planning/work.json` | Persistent planning-work cards. Retained. |
| `.kool-ade-packet/planning/archive/` | Superseded pre-modular specification, moved from `planning/archive/` without altering its contents. Historical evidence, not current authority. |
| `.kool-ade-packet/implementation/` | Implementation responses, corrections, reports, and verification evidence. Retained, including failed attempts. Machine-local state and locks remain ignored by Git. |
| `adr/` | Published implementation decisions. All three existing records reference commits reachable from this checkout and existing task files. Retained unchanged. |
| `docs/` | Maintainer guidance, runbooks, and explicitly historical acceptance evidence. |
| `SPECIFICATION.md` | Original MVP design reference. Historical scaffolding; current product authority is `planning/product/`. Existing source references still use its numbered sections. |

The obsolete archive location has been removed from this checkout. Packet now
writes new pre-modular archives into its workspace and relocates an existing
legacy archive on connection. Conflicting archives stop migration rather than
overwriting evidence. Git checkpoint retries include both sides of the move.

The implemented layout still distinguishes product specifications and repository
configuration from Packet's task and implementation workspace. Moving all of
`planning/` or `.planner/` requires a broader runtime-path migration: deleting
these directories would otherwise lose approval discovery, configuration, feature
reconciliation, and board history. No other reviewed file was established to be
disposable. Completed records and empty queues are not abandoned scratch files.

Frozen task contracts, workflow approvals, feature contracts, resolved outcomes,
and ADRs retain their historical text and paths. References inside those snapshots
describe their original repository revision; do not rewrite them as part of a
filesystem cleanup. Current documentation points to the relocated archive.

The ADRs use the `CHG-003-switch-workspaces` task batch. The active CHG-003 feature
document describes chat rendering. This pre-existing ID reuse does not establish
that chat rendering is complete: identify historical work by its full ticket path
and implementation commit. No feature was marked complete or re-approved during
this cleanup.
