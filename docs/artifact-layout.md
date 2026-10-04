# Repository artifact layout

Reviewed against the checked-out implementation on 2026-09-27. A connected
repository uses `.koolade-packet/` as the only live shared Kool.ad/e
project-artifact root.
Connection runs the restartable migration before loading planning state.

| Location | Purpose and ownership |
| --- | --- |
| `.koolade-packet/manifest.json` | Identifies the Kool.ad/e artifact format and product. |
| `.koolade-packet/config/project.md` | Project stakeholders, category owners, and identity fallback. |
| `.koolade-packet/config/repositories.json` | Stable repository IDs, remote mappings, and optional shared display names; no local checkout paths. |
| `.koolade-packet/config/mcp.json` | Locally stored MCP configuration; planner context receives server names only, not commands or credentials. |
| `.koolade-packet/planning/product/` | Current product specification, module manifest, and index. Six concepts are required; optional modules reflect actual project needs. |
| `.koolade-packet/planning/changes/` | Proposed feature/change specifications and their lifecycle. Merged behavior becomes current product truth only through reconciliation. |
| `.koolade-packet/planning/open-items.md` | Actionable questions, assumptions, ambiguities, ownership gaps, and reviews. |
| `.koolade-packet/planning/resolved-items.json` | Resolved item outcomes retained for board history and planning context. |
| `.koolade-packet/planning/decisions/` | Durable architectural decisions approved through planning. Routine choices and implementation reports do not create decision records. |
| `.koolade-packet/planning/tasks/` | Approved task stories, frozen feature snapshots, and task-batch metadata. |
| `.koolade-packet/planning/imports/` | Reference material explicitly imported by the operator. |
| `.koolade-packet/planning/archive/` | Superseded specification and historical implementation summaries preserved as evidence. |
| `.koolade-packet/state/workflow.json` | Feature approvals, task-batch references, and typed plan-comparison records (alternatives, recommendation, transcript, status, selection, and update time). A chosen plan is mirrored in its feature contract so approval fingerprints cover the adopted intent. |
| `.koolade-packet/state/work.json` | Persistent planning-work cards. |
| `.koolade-packet/implementation/` | Local implementation reports, responses, verification results, and recovery evidence. Git ignores these files; back them up separately when moving a live workspace. |
| `~/.koolade-packet/projects/<slug>/` or `$KOOLADE_HOME/projects/<slug>/` | Operator-local chat, task conversations, resumable generation checkpoints, and `repositories.json` mapping stable IDs to local checkout paths; not shared project truth. |
| Git common directory | Cross-process locks, migration/transaction journals, implementation queue and task state, private Git refs, and Pi event streams. |
| `docs/` and `SPECIFICATION.md` | Maintainer guidance and historical references. These are project files, not Kool.ad/e's live planning store. |

The old `planning/` and `.planner/` trees and `adr/` are migration inputs, not
live runtime locations. The migration maps legacy product modules into the six
required concepts while retaining project-specific modules, relocates feature
specifications, task stories, imports, open/resolved items, and archives, and
converts configuration and workflow state. Legacy `adr/implement-*.md` files
are historical implementation summaries and move byte-for-byte into the
implementation archive; actual approved decisions move to
`.koolade-packet/planning/decisions/`. The root `SPECIFICATION.md` is retained
as historical input. After migration, normal reads and writes use only the
canonical paths above.

Markdown artifacts are for people to read and edit. Versioned metadata and
structured workflow state determine feature lifecycle, approval, queue, and
implementation behavior; visible status text is rendered from that state and
cannot set it. Decision records under `.koolade-packet/planning/decisions/`
describe durable choices and their rationale. Implementation reports and
transcripts are evidence and belong under `.koolade-packet/implementation/` or
the historical archive, not in the decision record collection.

Migration preflights all sources and destinations before moving files. If a
legacy and canonical destination conflict, it preserves both and reports the
specific conflict instead of overwriting either copy. A private checkpoint lets
an interrupted migration resume safely. Legacy transaction recovery runs before
the old paths move, so rollback is confined to migration startup.

Do not delete Kool.ad/e-owned task records, resolved outcomes, or implementation
evidence merely because they are old or currently inactive. Task path changes
preserve stable identities and associated implementation state; historical task
snapshots remain useful for resuming and audit. Remove a file only after
confirming it is a disposable duplicate and updating every reference to it.

For a read-only task status audit, run
`cargo run --offline --example task_status -- /absolute/path/to/repository`.
The audit lists current board paths, preserved evidence identities, and totals.
