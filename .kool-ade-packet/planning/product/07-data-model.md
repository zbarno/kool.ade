## 7. Data Model

### 7.1 Artifact and storage layout

| Path / store | Content | Writer / owner |
| --- | --- | --- |
| `.kool-ade-packet/manifest.json` | Packet artifact-format and product identity | Artifact migration |
| `.kool-ade-packet/config/project.md` | Stakeholders, category owners, identity fallback | Validated settings/planning updates |
| `.kool-ade-packet/config/repositories.json` | Stable repository IDs, remote mappings, and optional shared display names | Repository settings |
| `.kool-ade-packet/config/mcp.json` | MCP configuration; prompts receive server names, while commands and credentials stay local | Operator settings |
| `.kool-ade-packet/planning/product/` | Current product modules, order/identity manifest, and index | Validated planner application |
| `.kool-ade-packet/planning/changes/CHG-*/` | Proposed feature intent and lifecycle | Validated planner application |
| `.kool-ade-packet/planning/open-items.md` | Actionable uncertainty with stable `CLR-` IDs and independent authority/priority | Validated item serializer |
| `.kool-ade-packet/planning/resolved-items.json` | Resolved questions and outcomes | Validated item serializer |
| `.kool-ade-packet/planning/tasks/<batch>/` | Approved task stories, frozen snapshots, and structured task metadata | Task generator and operator-approved edits |
| `.kool-ade-packet/planning/decisions/` | Material decisions approved through planning | Recoverable planning transaction |
| `.kool-ade-packet/planning/imports/` | Reference documents | Operator import |
| `.kool-ade-packet/planning/archive/` | Superseded product documents and historical implementation summaries | Artifact migration |
| `.kool-ade-packet/state/workflow.json`, `work.json` | Feature approvals, task batches, and persistent planning-work cards | Packet workflow |
| `.kool-ade-packet/implementation/` | Local reports, responses, verification results, and recovery evidence; ignored by Git | Implementation workers and Packet |
| `$PACKET_HOME/projects/<slug>/` | Operator-local chat, task conversations, and task-generation checkpoints | Local Packet process |
| `$PACKET_HOME/projects/<slug>/repositories.json` | Private mapping from stable repository IDs to verified local checkout paths | Repository settings/resolution |
| Git common directory | Queue/task state, locks, transaction journals, private refs, and Pi event streams | Packet workflow |

`.kool-ade-packet/` is the only live shared Packet project-artifact root. The
old `planning/`, `.planner/`, and `adr/` layouts are migration inputs only.
Migration moves and verifies their artifacts before removing the old copies. Old
implementation summaries move to the implementation archive; actual decision
records move to `.kool-ade-packet/planning/decisions/`. Root `SPECIFICATION.md`
is retained as historical input. See `docs/artifact-layout.md` for the path map
and conflict behavior. The project manifest holds logical identities and remote
information, not machine-specific checkout paths.
Optional `display_name` values are team-visible cosmetic labels stored with the
shared repository entry. Checkout paths remain in the private per-device map;
IDs and verified repository identity continue to drive routing and persistence.

### 7.2 Core types

`OpenItem` carries a stable ID, kind, category, priority, authority (`Agent`,
`Review`, `Human`), owner, question, reason, optional feature ID, recommendation,
evidence, structured decision brief where useful, and open/resolved status.
Agent-authority items can be resolved from evidence; Review items carry a
provisional direction; Human items preserve the user's choice. `TaskBatchRef` and
`BatchContract` freeze approved feature intent, affected product-module contents,
repository base commits, and configuration. Each story targets one logical
repository and carries dependency UIDs. Implementation records bind worktrees,
verification, recovery, pull-request/publication state, and merged commits to the
stable task UID.

### 7.3 Structured protocol

Wire responses have strict operation-specific schemas for planning, task outline,
task story, investigation, reconciliation, and decision explanation. Rust
normalizes accepted planning responses into an internal projection, then applies
the live-state, authority, approval, identity, and path checks before writing.
Document IDs are logical allowlisted values (`product:<module>` or
`feature:<change-id>`), never filesystem paths. Changed documents are complete
replacements. Legacy single-document response input remains confined to the
compatibility path for unmigrated repositories; modular updates cannot be mixed
with it (D-05 superseded by D-29).

### 7.4 Private and derived state

Local checkout paths are private and verified against the configured repository
identity before use. Main Chat and per-task conversations are stored locally and
remain separate; task histories never become another task's prompt. Activity,
generation checkpoints, and implementation reports support resume but are not
product authority. Repository surveys and any future full-text/vector indexes
are derived and disposable; a semantic match must be reopened at its source
before it supports a durable decision. Shared outcomes live in Git-backed
artifacts (D-07, ratified by D-31).

### 7.5 Configuration and automation

The connected Git identity precedes the configured identity fallback (FR-13,
D-14). Category ownership is person, group, or seat-inherited. `PACKET_HOME`
changes private storage; `PACKET_PI_BIN` selects the Pi executable, and
`PACKET_TURN_TIMEOUT_SECS` sets the per-turn deadline. The project automation
policy stores Auto Plan, Auto Build, and Auto Publish separately: planning and
building are enabled by default, while automatic publication is disabled by
default. The system Git CLI performs fetch and publication operations.

No collaboration wire payload is current (D-18, D-22). If that channel ships,
only planning artifacts and presence are shared; Main Chat and task histories
remain local (D-31). D-32 fixes the future direct peer topology and repository
access trust ground; D-33 requires a convergence layer for true concurrent
co-authoring. Those channel designs remain deferred.
