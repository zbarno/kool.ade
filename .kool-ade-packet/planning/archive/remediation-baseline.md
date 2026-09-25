# Packet Hardening Remediation Baseline

Recorded 2026-09-25 against clean commit `eb001368949d29cfc45f706c1343a5b24b2ce145`, before this remediation changed production code. Rust and Cargo are 1.98.1; Clippy is 0.1.98. The full source requirements are in [the remediation plan](../tasks/packet-hardening-architecture-remediation/source-plan.md), and work status is in its [phase ledger](../tasks/packet-hardening-architecture-remediation/README.md).

## Verification baseline

- `cargo test --offline --all-targets --quiet -- --test-threads=1`: passed; 484 library tests, zero failures. The all-target run also executes the multi-repository and task-workflow integration tests; one migration test is intentionally ignored unless explicitly opted into. Example targets contain no tests.
- `cargo fmt --check`: failed with 482 diff sections across 33 Rust files. The tree was left unchanged; formatting the whole repository is a later remediation, not part of establishing this baseline.
- `cargo clippy --offline --all-targets`: exit 0 with existing warnings. On this toolchain the summaries are 96 library warnings; 111 library-test warnings (95 duplicates); and one warning each in `multi_repository_feature` and `task_workflow`. The run has 24 distinct Clippy lint names. This is the warning baseline to replace with zero warnings by the quality-gates phase.
- `git status --porcelain`: empty at the baseline commit.

## Current artifact tree

| Scenario | Observed starting behavior |
| --- | --- |
| Fresh repository | `PlannerState::bootstrap_missing()` creates `planning/specification.md`, `planning/open-items.md`, and `.planner/config.md` (pinned by `core::state::tests::bootstraps_skeletons_and_reports_creations`). Task batches are routed to `.kool-ade-packet/planning/tasks/` only when `planning/tasks/` is absent. Implementation state/evidence is under `.kool-ade-packet/implementation/`. There is no artifact manifest. |
| Packet repository | Shared current product docs and items are at root `planning/` (13 product modules, index, open items, resolved items); project config and workflow are under `.planner/`; three implementation ADRs are in root `adr/`; `SPECIFICATION.md` remains at root. `.kool-ade-packet/` contains five task batches, an archived product-spec snapshot, and local implementation state/evidence. Canonical `.kool-ade-packet/config/`, `state/`, `planning/changes/`, and `planning/decisions/` are absent. |
| Repository with legacy `planning/` | Root planning paths remain live. Product modules are read and updated in place; `planning/tasks/` wins the task-directory selection when present. No whole-tree migration to `.kool-ade-packet/` occurs. Product-spec migration tests cover the narrower historical single-file-to-modules migration. |
| Repository with legacy `.planner/` | `.planner/config.md`, `.planner/mcp.json`, and `.planner/workflow.json` remain the active config, MCP, and workflow paths; the directory is not migrated. |
| Repository with existing task batches | The Packet checkout has five registered batches with 24 stories total: MVP (7), Board Card Display (3), Readable Chat Replies (5), Editable Operator Persona (5), and Switch Workspaces (4). Batch registrations live in `.planner/workflow.json`; story files live under `.kool-ade-packet/planning/tasks/`; per-run implementation state is separate under `.kool-ade-packet/implementation/`. |

The tree confirms the remediation starts from a mixed layout. Legacy locations are still runtime inputs and, in several cases, runtime destinations. Do not relocate or delete artifacts until the canonical owner, migration plan, and conflict behavior are established.
