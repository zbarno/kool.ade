# Repository artifact layout

Kool.ad/e keeps project artifacts in a separate, managed planning Git repository. A code checkout is never used as a planning root. The planning repository working-tree root is the artifact root.

For a GitHub code repository `owner/name`, setup uses the paired remote `owner/name.packet`. It clones both repositories into Kool.ad/e-managed locations. If the planning repository is missing, Kool.ad/e creates and pushes it with the code repository's visibility. Existing repositories with mismatched visibility are rejected; Kool.ad/e does not change visibility.

```text
~/kool-ade-workspaces/<code-repository>/
~/.koolade-packet/projects/<workspace-uuid>/
├── association.json
└── planning-repo/
    ├── manifest.json
    ├── config/
    ├── planning/
    └── state/
```

`KOOLADE_HOME` can replace `~/.koolade-packet`. Local workspaces and planning checkouts are named by the workspace UUID, not by the current code branch or an arbitrary folder chosen during setup.

## Shared planning repository

| Path | Purpose and ownership |
| --- | --- |
| `manifest.json` | Workspace identity and schema. |
| `config/project.md` | Project description and planning context. |
| `config/repositories.json` | Stable code repository IDs and remotes; it contains no local checkout paths. |
| `planning/product/` | Current product specification, module manifest, and index. |
| `planning/changes/` | Proposed feature specifications and their lifecycle metadata. |
| `planning/items/` | Human-readable question and decision content. |
| `planning/decisions/` | Durable architectural decisions approved through planning. |
| `planning/tasks/` | Approved task stories and frozen feature snapshots. |
| `planning/imports/` | Reference material explicitly imported by the operator. |
| `planning/archive/` | Historical planning material retained for reference. |
| `state/` | Structured feature, task, item, and planning-work records. |

Markdown stays human-readable. Structured metadata and state determine workflow behavior; visible status text does not grant approval or select a plan. Shared changes are synchronized through Git. A branch on the code repository does not change planning authority.

## Operator-local state

Conversation history, task conversations, resumable generation checkpoints, and other private application state live under `KOOLADE_HOME`. Paths for Kool.ad/e-managed code clones live in the workspace association, not in the shared planning manifest. Setup does not accept user-selected local checkout paths. Credentials and provider configuration are never written to shared planning files.

Implementation reports, verification results, task clones, process locks, and recovery state remain in Kool.ad/e's local application data or the Git common directory. They are not shared project planning artifacts.

## Unsupported embedded layout

An embedded `.koolade-packet/` directory inside a code checkout is not a supported project-artifact layout. Kool.ad/e does not discover, load, migrate, or use it as a fallback. To use a code repository, open it through the managed GitHub clone flow and its paired planning repository. Existing contents are left untouched.

For schema, identity, synchronization, and recovery rules, see [Planning repository v1](planning-repository-v1.md).
