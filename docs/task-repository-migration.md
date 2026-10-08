# Task repository migration checklist

Issue: [#85](https://github.com/zbarno/kool.ade/issues/85)

## Migration decision

New tasks use full independent clones under Kool.ad/e's private project state and an app-owned bare repository cache. Existing saved task records keep their current linked worktree path and use a narrowly scoped legacy execution path until those tasks complete. The decoder preserves the old path and marks the record as legacy; migration never copies, resets, or discards partial work.

## Checklist

- [x] Inventory execution, verification, reconciliation, resume, publication, cleanup, sandbox, UI, docs, and tests for worktree assumptions.
- [x] Persist task repository kind, repository identity, selected source ref, exact source SHA, cache path, and execution path.
- [x] Seed and refresh an app-owned bare cache without writing to the user checkout.
- [x] Create full per-task clones from the cache; reject alternates and shared Git metadata.
- [x] Run Bubblewrap with the task clone as its writable root and retain the existing sandbox restrictions.
- [x] Resume dirty task clones in place and keep interrupted clones during cleanup.
- [x] Reconcile destination movement from the persisted source SHA in an independent clone.
- [x] Clean completed clones only after durable publication and verified evidence retention.
- [x] Keep legacy linked-worktree behavior reachable only for records written by older versions.
- [x] Update prompts, UI labels, README, and task failure documentation.
- [x] Add regression coverage for dirty checkouts, exact SHA, local-only refs, clone isolation, resume, integration, publication, and cleanup.
- [x] Search the full repository for remaining worktree references and classify every survivor.

Active prompts, status messages, security text, and product copy describe independent task repositories. Remaining source references are compatibility and implementation details: the state decoder reads the old `worktree` field and labels that record `legacy_worktree`; recovery accepts schema 1 snapshots with a `worktree` field only for those legacy records, while new snapshots use schema 2 and `task_repository`; legacy-only execution, reconciliation, integration, and cleanup retain linked-worktree operations; Git's `is-inside-work-tree`, generic filesystem helper names, and the example error refer to Git working trees; and task-failure notes describe historical linked-worktree cleanup.
