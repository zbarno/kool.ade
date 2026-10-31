# Planning store access audit

Shared planning data is owned by a managed `PlanningStore`. Its root is the standalone planning Git checkout. The code checkout remains available for source inspection, Git identity, and task execution, but it is never mapped to a planning store.

## Access boundaries

- `PlannerState::load_with_store` and the store-backed workflow APIs read and write planning files beneath the selected planning repository root.
- Code checkout scans and task execution use the managed code checkout. Planning artifacts are not created there.
- Workspace associations map a workspace UUID to its managed planning checkout and code repository IDs to local checkout paths. Local paths are not written to the shared planning manifest.
- Conversation history, private execution state, and machine setup data live under `KOOLADE_HOME` or the platform user-data directory. This machine-local state root is not the shared project-artifact root.
- Atomic batch writers may stage files temporarily, then install final files through planning-store transactions.

## Unsupported path behavior

The application does not expose a legacy adapter that maps a code checkout to an embedded `.koolade-packet` store. Connection requires an existing managed workspace association. The GitHub setup flow clones the code repository, opens its same-owner `<repository>.packet` planning repository, or creates the missing planning repository with matching visibility. It never browses for or adopts arbitrary local code or planning checkouts.

Embedded project artifacts are not discovered, read, migrated, or used as fallback. Existing files in a code checkout are left untouched. The similarly named directory under the user's home is only machine-local application data.

## Reproducible source audit

```sh
rg -n 'PlanningStore::legacy_embedded|LegacyPacketSource|legacy_embedded|map_local_checkout|existing_checkout_mapping' src --glob '*.rs'
```

The command should return no production API or implementation path. Test-only assertions may mention the former directory name to ensure setup does not create it.
