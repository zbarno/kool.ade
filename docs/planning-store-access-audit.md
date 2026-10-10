# Planning store access audit

Shared planning data is owned by `PlanningStore`. Managed callers receive one store instance and use root-relative paths; the code checkout stays available separately for source inspection and Git identity. The legacy `PlanningRoot` adapter maps a code repository path to its existing `.koolade-packet` directory without moving files.

## Managed planning reads and writes

- `PlannerState::load_with_store` loads product documents, workflow, items, project configuration, and repository mappings from the injected store. `PlannerState::load` remains the legacy compatibility entry point.
- Turn application, product updates, approvals, task generation, project repository updates, cancellation records, imports, planning work, and background task conversations use the `PlanningStore` held by `PlannerState`.
- Managed text imports are read through the selected store and included, with a prompt size limit, in planning context. Import directory listing rejects symlinked parents and entries before reporting names or sizes.
- Code checkout scans and task execution continue to use the code repository root. Managed task stories are read from the store for planning and approval; implementation clones receive task content without write access to that store.

## Remaining embedded or repository-local paths

| Location | Reason it remains | Boundary |
| --- | --- | --- |
| `src/artifacts/planning_store/root.rs`, `src/core/state.rs`, and the legacy overloads in `src/core/attention.rs`, `src/core/implementation/`, and `src/core/time_accrual/host.rs` | Backward-compatible entry points for callers that still provide a code repository path. They construct `LegacyEmbedded`; application paths that have project state pass the injected store overloads. | No managed path is derived from the process working directory. |
| `src/app/welcome/connect.rs` and `src/app/welcome/repository_picker.rs` | Existing-project onboarding still discovers legacy embedded artifacts so old projects can be opened. Managed create/join flows are introduced by the dependent repository-lifecycle issue. | This path is only for the legacy connection workflow. |
| `src/artifacts/migration.rs`, `src/artifacts/migration/**`, and `src/artifacts/migration/product.rs` | Migration discovery, verification, recovery, and rollback must read and preserve the old embedded source. The compatibility bootstrap wrapper also supports legacy callers. | Migration retains the original tree as recovery evidence; it does not make the old root the authority for a managed project. |
| `src/artifacts/transaction.rs::apply` | Public legacy transaction adapter for callers that still pass a code repository path. The current runtime uses `PlanningStore::transaction`; direct adapter calls remain in compatibility tests. | Writes only beneath the supplied repository's `.koolade-packet` root. Managed callers use `apply_store` with validated store-relative paths. |
| `src/artifacts/mcp_io.rs`, `src/core/context_build/mcp.rs`, and the MCP settings dialog | MCP command configuration is operator-local and may contain credentials. It is deliberately excluded from `PlanningStore` paths. | `.koolade-packet/config/mcp.json` is ignored by Git. |
| `src/artifacts/time_ledger.rs` and `src/core/time_accrual/host.rs` | Time accrual is local telemetry, not shared planning authority. | `.koolade-packet/state/time-ledger.log` is excluded from managed-store resolution and ignored by Git. |
| `src/core/implementation/state_paths.rs`, `src/core/implementation_queue/state.rs`, `src/core/attention.rs`, and `src/persistence/` | Implementation state, queue locks, blocker sidecars, and conversations are operator-local execution state. Existing embedded implementation records remain readable for recovery. | Current records live under the operator's Kool.ad/e data directory or the local Git common directory; they are not copied into the planning repository. |
| Temporary staging in `src/artifacts/task_docs/batch_write.rs` and `src/artifacts/task_docs/generation.rs` | Batch files are built atomically in a temporary directory before the store transaction installs them. | Final planning files are written through `PlanningStore` transactions. |
| `src/artifacts/layout.rs`, path-normalization adapters, user-facing compatibility text, and test fixtures | Legacy constants and sample project paths remain necessary for existing projects and tests. | Runtime prompts derive product, configuration, import, and change paths from the selected store. |

## Reproducible grep audit

The source-level audit searched these remaining path and write primitives:

```sh
rg -n 'PlanningStore::legacy_embedded|ArtifactLayout::new|repo_artifact|transaction::apply|write_planning|atomic_write|atomic_create|std::fs::write|fs::write' src --glob '!**/tests/**'
```

Matches inside `#[cfg(test)]` blocks and test-only files are fixtures. Production matches are classified above: store-backed planning transactions, explicit legacy migration/compatibility, operator-local settings or execution state, and temporary staging. No unclassified shared planning writer remains in the managed application path.
