# Planning repository v1

This is the normative on-disk contract for managed planning repositories.
Every workspace has one separate planning repository. Setup starts from a
GitHub code repository and derives a same-owner `<code-repository>.packet`
planning repository. If that repository is missing, Kool.ad/e creates it with
the code repository's visibility; an existing repository with a different
visibility is rejected. Kool.ad/e clones both repositories into managed
locations. It does not accept arbitrary local code or planning checkout paths.
Embedded `.koolade-packet/` project artifacts are unsupported: they are not
read, migrated, or used as fallback, and existing contents are left untouched.

## Workspace identity and ownership

A **workspace** is the durable planning and coordination boundary. Each
workspace has exactly one authoritative planning Git repository and zero or
more code repositories. A code repository is an optional implementation
target; adding one never creates another planning repository. A workspace with
no code repositories is valid and can still contain product documents,
questions, decisions, and planning work.

The workspace ID is a UUID generated once and stored in the planning
repository's `manifest.json`. It is independent of the display name, Git URL,
code repository IDs, filesystem paths, and active branches. Every operator
joining the same planning repository uses that same ID. Local checkout paths
and user state are independent per operator.

`workspaceId` defines the logical workspace. The application derives the
planning remote from the code remote, then validates the planning manifest
and Git history before associating it. Remote URLs are locators and may be
aliases, so URL equality alone neither proves nor disproves identity. An
existing local workspace is reopened through its saved workspace UUID and
association; setup does not browse for a replacement checkout or expose a
reconnect action. A fork retaining the UUID is a replica, not a second
workspace. The application does not silently switch an established
association to a different history.

A feature/change belongs to the workspace and may cover several code
repositories. An executable task names one code repository; a planning-only
task may be unbound. `repositoryId`, `sourceBranch`, and `destinationBranch`
identify the task's target repository and its source/destination branches.
Those branches always belong to that code repository, never to the planning
repository. A feature spanning repositories is represented by tasks routed to
the relevant repository IDs, each with its own branch targets.

Task, feature, decision, and planning-work UUIDs are durable identities. A
move, rename, branch switch, or display-ID change does not create a new
identity. Migration preserves valid existing UIDs; it does not regenerate
them. A legacy record without a UID receives one once during migration, which
must persist it before retry or publication. If two records in one workspace
claim the same UID but have different content or meaning, migration stops and
reports the collision; it never merges or renumbers them silently. Human IDs
and filenames are labels and paths, not identity keys.

For compatibility, a legacy manifest or association may supply `projectId`
as the workspace ID when it is a valid UUID and `workspaceId` is absent. New
manifests and associations write only `workspaceId`. If both fields are
present, they must match. Existing task and planning-work UIDs remain
unchanged when the older `projectId` name is replaced by `workspaceId`.

## Shared planning repository

The planning repository working-tree root is the artifact root. There is no
nested `.koolade-packet/` directory in a managed planning repository. The
checkout lives at:

```text
$KOOLADE_HOME/projects/<workspace_uuid>/planning-repo/
```

`KOOLADE_HOME`, when set to a non-empty value, is the operator-local state
root. Otherwise the existing per-user home default is used (`$HOME/.koolade-packet`
or the platform's user-profile equivalent). The managed path is derived from
that root and the workspace UUID; it must not depend on the process working
directory. An existing operator directory may keep its legacy project-based
name during compatibility migration, but that name is not workspace identity.

The tracked v1 layout is:

```text
planning-repo/
  manifest.json
  config/
    project.md
    repositories.json
  planning/
    product/
      index.md
      manifest.json
      ... product modules ...
    changes/
      <feature-uid>/
        specification.md
        metadata.json
    items/
      <item-uid>.md
    tasks/
      <task-batch-or-feature-scope>/
        README.md
        specification.md
        <task-id>.md
        workflow/
          <task-uid>.json
    decisions/
      <decision-id>.md
    imports/                 # optional, operator-imported references
    archive/                 # optional, preserved historical material
  state/
    work/
      <work-uid>.json
    workflow/
      <feature-uid>.json
    items/
      <item-uid>.json
    tasks/
      <task-uid>.json
```

The required tracked layout has `manifest.json`, `config/project.md`,
`config/repositories.json`, `planning/product/**`, and these record
collections: `planning/changes/<feature-uid>/specification.md`,
`planning/changes/<feature-uid>/metadata.json`, `planning/tasks/**`,
`planning/decisions/**`, `planning/items/<item-uid>.md`,
`state/items/<item-uid>.json`, `state/work/<work-uid>.json`,
`state/workflow/<feature-uid>.json`, and `state/tasks/<task-uid>.json`.
Initialize `planning/product/index.md` and `planning/product/manifest.json`.
Collections may have zero records; create their directories and record files
as records are added, without placeholder records. `planning/imports/**` and
`planning/archive/**` are optional shared material.
Legacy `planning/open-items.md`, `planning/resolved-items.json`,
`state/workflow.json`, and `state/work.json` are compatibility and migration
inputs only. They are never authoritative shared files in a new managed
planning repository. Managed workspaces read and write planning through the
injected `PlanningStore`. These record-format upgrades operate only inside the
standalone planning repository; they do not inspect code checkouts.

### Per-record authority and revisions

Each independently mutable shared record has its own file and stable ID:

| Record | Authoritative file | Responsibility |
| --- | --- | --- |
| Feature/change content | `planning/changes/<feature-uid>/specification.md` | Human-readable feature scope and acceptance intent. |
| Feature metadata | `planning/changes/<feature-uid>/metadata.json` | Stable feature UID and descriptive feature metadata or references; lifecycle, approval, and selected-plan state belong to the feature workflow record. |
| Planning work | `state/work/<work-uid>.json` | One existing `Work` record, retaining its UID, kind, relationships, source branch, destination branch, and other durable fields. |
| Feature workflow | `state/workflow/<feature-uid>.json` | One feature's approval, selected-plan fingerprint, review, and reconciliation state. |
| Planning item content | `planning/items/<item-uid>.md` | Human-readable question, ambiguity, assumption, finding, evidence, or recommendation. |
| Planning item state | `state/items/<item-uid>.json` | One item's UID, kind, status, timestamps, owner, blockers, feature/task links, and revision. |
| Task planning document | `planning/tasks/<scope>/**` | Human-readable task batch, specification, and story documents. |
| Task workflow attachment | `planning/tasks/<scope>/workflow/<task-uid>.json` | Per-task workflow metadata attached to its planning documents; it does not duplicate the task status record. |
| Task status | `state/tasks/<task-uid>.json` | Durable, user-visible task status and publication state, plus only the progress summary needed by shared planning views. |

Live worker state, streaming progress, attempt logs, conversations, and
implementation reports remain operator-local. The shared task status record
supports planning views and coordination; it is not a live execution log or
transcript.

Every per-record JSON file carries its stable record ID, a schema or
compatibility version, a record revision, and the timestamps needed for sync
and conflict diagnostics. The ID in the record must agree with the UID in its
path. A record contains only data for that ID. Preserve current Work, task,
feature, item, and approval identities and relationships through conversion;
never regenerate an ID because a record moved or changed format.

Board columns, counts, filters, item summaries, and other views are derived
from these record files. Any cache or small index is explicitly derived,
rebuildable, and non-authoritative. Missing derived data can be reconstructed
without losing planning state. Do not create an authoritative replacement
blob or a giant index for the record tree.

A write names the target record ID and the expected revision read by the
operator. Applying a stale revision for the same ID is rejected or surfaced
as a record conflict; it never silently overwrites that record. Changes to
different record IDs can coexist in a collection. Git retries replay the
semantic operation against those IDs and their expected revisions rather
than treating a whole-file snapshot or text merge as the authority. A
malformed record is reported with its path and does not authorize silently
skipping it or losing other records.

For example, operator A can advance `state/work/<work-a-uid>.json` from
revision 4 while operator B advances `state/work/<work-b-uid>.json` from
revision 7. Both updates can be replayed and committed because they target
different IDs. If both operators edit the same work UID from revision 4, the
second update must be rejected or surfaced as a conflict for that UID; it
cannot replace the first update based on commit time.

The planning manifest is the identity record for the standalone planning
repository. Machine-local application data has its own storage and is not a
planning manifest. The planning manifest schema is:

```json
{
  "schemaVersion": 1,
  "workspaceId": "1d5b4b9e-1234-4f56-8123-ccfa6ad17910",
  "kind": "koolade-planning",
  "canonicalRef": "refs/heads/main"
}
```

`workspaceId` is a canonical hyphenated UUID. `kind` is exactly
`koolade-planning`. `canonicalRef` is a fully qualified local branch ref of
the form `refs/heads/<branch>` that names the planning repository's shared
coordination branch. Validate it as a Git ref; reject short names, tags,
remote-tracking refs, `HEAD`, revision expressions, and invalid ref
components. The ref identifies planning history only. It does not choose or
change a code repository's source-branch default.

`config/repositories.json` remains the shared registry of stable code
repository IDs and portable remote identities. It contains no local checkout
paths or credentials. Existing IDs, including `root`, remain stable during
migration and continue to identify the same code repository. The ID `root` is
not an instruction to create a code repository; new workspaces may have an
empty registry. The existing optional `display_name` field is display
metadata, not identity. Local paths belong only in the association below.

## Operator-local association and private state

Each operator binds local directories to the shared workspace in a file that
is never committed:

```text
$KOOLADE_HOME/projects/<workspace_uuid>/association.json
```

The v1 association shape is:

```json
{
  "schemaVersion": 1,
  "workspaceId": "1d5b4b9e-1234-4f56-8123-ccfa6ad17910",
  "planningRepoPath": "/operator-data/projects/1d5b4b9e-1234-4f56-8123-ccfa6ad17910/planning-repo",
  "codeCheckouts": [
    { "repositoryId": "root", "path": "/workspaces/storefront" }
  ]
}
```

`planningRepoPath` and checkout paths are absolute paths on that operator's
machine. Code paths are recorded by managed setup and accepted only when
their Git remotes match the shared registry. Opening a workspace uses its
saved association; setup does not browse for alternate local paths. The
shared planning remote, not local paths, is the workspace identity. MCP
context continues to receive server names only; commands and credentials
remain local.

The following data is private to the operator and is not tracked in the
planning repository:

| Data | v1 local location or handling |
| --- | --- |
| Workspace association and local code checkout paths | `projects/<workspace_uuid>/association.json` |
| Managed planning checkout | `projects/<workspace_uuid>/planning-repo/` |
| Implementation records and verification evidence | Operator-local Kool.ad/e application data; not part of the planning Git repository |
| Time ledger, MCP configuration, and other local settings | Operator-local Kool.ad/e application data; never shared as planning truth |
| Existing `$KOOLADE_HOME/projects/<repo_slug>/repositories.json` checkout map | `projects/<workspace_uuid>/association.json` under `codeCheckouts`; local paths are never copied into shared `config/repositories.json` |
| Chat histories, task conversations, checkpoints, telemetry, and other user state | Operator-local Kool.ad/e application data; never shared as planning truth |
| Task clones, local locks, worker state, run logs, and Pi event streams | Operator-local runtime state (including Git common-directory state); do not commit or treat it as shared workflow |
| Git credentials | Existing host Git credential mechanism; never copy into a manifest, association, planning document, prompt, or log |

Kool.ad/e does not copy or migrate project artifacts from a code checkout into
the planning repository. Missing private application data is reported through
its own storage errors; it does not authorize reading project files from the
code checkout as a substitute.

## Ownership, synchronization, and offline behavior

| State | Authority and synchronization |
| --- | --- |
| Manifest, project description, repository registry, product documents, changes, task documents, decisions, and per-record item/work/feature-workflow/task-state files | Shared planning Git repository. A successful commit on `canonicalRef` is the durable shared update. |
| Board columns, counts, filters, summaries, and other projections | Derived from authoritative per-record files; any stored cache is rebuildable and is not shared authority. |
| Association, local paths, chats, MCP configuration, implementation evidence, time ledger, task clones, worker processes, and local queue locks | One operator's local state. These do not become shared by committing planning files. |
| Execution lease | Temporary remote Git ref in the planning repository's shared remote, keyed by both workspace UUID and task UID. It arbitrates who may execute a task; it is not planning content or approval. |

A planning write is one application-owned transaction over the named record
IDs and any related content documents. The writer reads and validates each
record's expected revision, then records the logically related changes in
one Git commit. Synchronization checks the remote `canonicalRef` before
updating it. If another operator advanced it, replay the semantic operation
against the affected IDs: unrelated record updates may coexist, while a
changed revision for the same ID is reported as a conflict for explicit
resolution. Never use whole-file text merge or last-writer-wins as the
authoritative rule, and never publish only part of a logical record update.
Git author metadata is not proof of workspace membership, user identity, or
approval. Planning text alone cannot authorize an approval or publication.

Shared execution leases use the planning remote as the workspace-wide
arbitration point, even when tasks target different code remotes. The key is
workspace-qualified, equivalent to
`refs/heads/koolade/claims/<workspace_uuid>/<task_uid>`. Its payload records
the workspace and task UIDs, operator/session UUID, code repository ID, base
commit SHA, and UTC issue and expiry times. Acquisition has one winner;
renewal and stale takeover compare against the current ref. A worker verifies
the planning lease before and after publication. When the planning and code
push destinations are the same Git remote, the candidate branch and claim
renewal are pushed atomically. With different remotes, a takeover can race an
accepted code push. Each code push updates a session fence and candidate branch
in one atomic code-remote transaction. The worker rechecks its lease and uses
the session fence plus the exact branch SHA to compare-and-swap its candidate
back to its previous value after lease loss. If a newer owner has fenced the
branch or another writer has replaced it, cleanup preserves the newer ref and
reports that recovery may be needed. An ambiguous push response is checked
against both remote refs; if the code remote cannot be reached to verify or
retract an accepted candidate, the branch can remain and recovery may be
needed. This cross-remote cleanup cannot make the two remotes one atomic
transaction, and an accepted branch can be briefly visible before lease loss
is detected. A lost lease blocks PR creation when observed before the API
request; the code host request itself cannot be transactionally fenced against a
simultaneous takeover. Existing isolated code clones remain the execution
environment. Legacy task-only claims on code remotes are not shared planning
authority, but remain usable while an existing worker is running under them.

The current legacy claim ref is
`refs/heads/koolade/claims/<task_uid>` on the task's code-repository remote.
Its record contains the owner, session ID, base commit, and `claimedAt`; the
current worker refreshes it every five minutes and treats it as stale after
fifteen minutes. During cutover, an existing worker keeps using that ref for
refresh, publish fencing, and release. Migration does not move, delete, or
rewrite a live claim. Switching a workspace to planning-remote claims is
allowed only at a coordinated quiescent boundary: older writers are stopped,
and every registered code remote is reachable and has no live legacy claim.
A live claim is left to its owner to release normally; an expired claim uses
the existing explicit, observed-session compare-and-swap recovery before it
can be retired. If a remote is unavailable, a record is malformed, or an
active worker cannot be fenced, cutover stops and preserves the record and
clone. New workspace-wide claims begin only after this barrier. If both a
legacy code-remote ref and a workspace planning-remote ref exist for one task,
fail closed: do not start or publish, keep both refs, and require an explicit
compare-and-swap resolution. Never choose one by timestamp or delete the other
as cleanup.

When the shared remote is unavailable, edits in the local planning checkout
remain local and are visibly pending synchronization. The application must
not claim another operator can see them. Shared-mode automatic execution is
not allowed without the global lease; a manual local-only start requires an
explicit operator choice and warning, and does not claim global exclusivity.
Workspaces that are deliberately local-only use local process locks. Git
remote write permission does not provide per-field access control or a
cryptographically trustworthy operator identity. The v1 contract does not
promise server-side arbitration, automatic real-time notification, or
conflict-free concurrent edits.

## Discovery and association errors

Discovery follows the managed GitHub setup flow and never silently forks
workspace identity:

1. **Clone code repository** accepts a GitHub code URL, clones it into the
   managed code-checkout folder, and derives `<owner>/<repository>.packet`.
2. **Open paired planning repository** clones the derived repository if it
   exists. Its visibility must match the code repository's visibility.
3. **Create missing planning repository** creates the derived remote with the
   code repository's visibility, pushes the initialized planning store, and
   records the paired code remote.
4. **Open existing local workspace** resolves the saved workspace UUID and
   the managed checkouts from its operator-local association. An arbitrary
   code checkout cannot create or replace a workspace association.
5. Validate the association and planning manifest before loading shared
   state. Do not fall back to project files inside the code checkout or create
   a replacement workspace when an association is broken.

The following messages are part of the connection contract; `<...>` values
are substituted with the relevant UUID, path, or reason:

| Condition | Message |
| --- | --- |
| Selected planning root has no manifest | `Planning repository is missing manifest.json.` |
| An existing association points to a missing planning checkout | `Planning repository <path> from workspace association <association_path> is missing; restore it through the managed setup flow. Kool.ad/e will not create a replacement.` |
| Planning checkout exists but is unreadable | `Planning repository <path> cannot be read: <reason>.` |
| Planning manifest is malformed or violates the v1 schema | `Planning repository manifest <path> is invalid: <reason>.` |
| A known workspace has no local binding | `No workspace association exists for <workspace_id>. Start from its paired code repository in the GitHub setup flow to create a managed local association.` |
| Association cannot be parsed or validated | `Workspace association <path> is invalid: <reason>.` |
| Association and planning manifest name different workspaces | `Workspace ID mismatch: association names <expected_id>, planning repository names <actual_id>.` |
| A Git operation is already holding the planning checkout lock | `Planning repository <path> is locked by another Git operation; finish or recover that operation before retrying.` |
| The selected directory is a different existing Git repository | `Directory <path> already contains an unrelated Git repository; nothing was overwritten.` |
| A normalized record is malformed or its path ID differs from its record ID | `Planning record <path> is invalid: <reason>.` |
| Same workspace ID appears at an unrelated or diverged planning history | `Workspace <workspace_id> has a conflicting planning history at <selected_path>; refusing to replace <existing_path>.` |
| Local checkout remote differs from the shared registry | `Code repository <repository_id> origin does not match its registered remote.` |
| Legacy and workspace leases both exist for a task | `Execution lease conflict for task <task_uid>: legacy code-repository claim and workspace planning claim both exist; preserve both and resolve before retry.` |

Errors leave all existing files and associations intact. A malformed planning
manifest leaves the managed checkout and association untouched. Kool.ad/e does
not search code checkouts for embedded project data or create a replacement
workspace as a fallback.

## Worked examples

### Workspace with no code repositories

This is a valid planning-only workspace. No implicit `root` code checkout is
created:

```json
{
  "repositories": []
}
```

The workspace can hold its product specification, decisions, questions, and
planning work. Tasks remain unbound until an operator registers a code
repository and explicitly routes executable work to it.

### One code repository

The planning repository is separate from the application repository. `root`
is retained here because it is the stable ID already present in this example
workspace; it is not a special planning-store path.

```json
{
  "repositories": [
    {
      "id": "root",
      "role": "Storefront",
      "remote": "https://github.com/example/storefront.git"
    }
  ]
}
```

A task with `repositoryId: "root"`, source branch `feature/checkout`, and
destination branch `main` runs against the local storefront checkout. Its
planning files and approval state remain in the separate planning repository.

### Feature across three code repositories

```json
{
  "repositories": [
    { "id": "root", "role": "Web", "remote": "https://github.com/example/web.git" },
    { "id": "api", "role": "API", "remote": "https://github.com/example/api.git" },
    { "id": "infra", "role": "Infrastructure", "remote": "https://github.com/example/infra.git" }
  ]
}
```

One feature specification can cover all three repositories. Its executable
tasks identify `root`, `api`, or `infra` separately, and each task's branch
targets are validated against that repository. A task without a local checkout
can still be planned and reviewed but cannot run against a missing code tree.

### Two operators joining one workspace

Both operators connect to the same planning Git remote and therefore read the
same `workspaceId` and task UIDs. Their associations contain different local
paths and are never committed:

```json
{
  "schemaVersion": 1,
  "workspaceId": "1d5b4b9e-1234-4f56-8123-ccfa6ad17910",
  "planningRepoPath": "/operator-a/data/planning-repo",
  "codeCheckouts": [
    { "repositoryId": "root", "path": "/operator-a/src/web" },
    { "repositoryId": "api", "path": "/operator-a/src/api" }
  ]
}
```

Operator A stores this file at
`$KOOLADE_HOME/projects/<workspace_uuid>/association.json`.

```json
{
  "schemaVersion": 1,
  "workspaceId": "1d5b4b9e-1234-4f56-8123-ccfa6ad17910",
  "planningRepoPath": "/operator-b/data/planning-repo",
  "codeCheckouts": [
    { "repositoryId": "root", "path": "/operator-b/work/web" }
  ]
}
```

Operator B stores this file at
`$KOOLADE_HOME/projects/<workspace_uuid>/association.json`.

Operator B sees the shared API tasks but cannot execute one until Kool.ad/e
clones the API repository into that operator's managed workspace. Different
managed clone locations do not create different workspaces or alter shared
planning state.

### Two code branches

The planning repository remains on its canonical planning ref (for example,
`refs/heads/main`) while a task targets branches in the API code repository:

```yaml
repositoryId: api
sourceBranch: feature/faster-checkout
destinationBranch: release/1.2
```

Switching the local API checkout between branches does not change the
workspace ID, planning repository, task UID, or planning authority. Branch
names are interpreted only within `api`; another code repository may have
branches with the same names without ambiguity.

## Unsupported embedded project layout

The standalone planning repository is the only live shared project-artifact
root. A code checkout is source code only. Kool.ad/e does not search for
`.koolade-packet/`, `.koolade/`, `.planner/`, or other legacy planning roots
inside a code checkout. It does not migrate those files, merge them into a
planning repository, or fall back to them when a managed association is
missing or damaged. Existing files are left untouched.

Schema and record normalization inside an already-managed planning repository
remain separate concerns; they never cause Kool.ad/e to adopt or read an
embedded code-checkout artifact tree. For current file ownership, see
[Repository artifact layout](artifact-layout.md).
