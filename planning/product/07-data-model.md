## 7. Data Model

### 7.1 Artifact and storage layout

| Path / store | Content | Writer / owner |
| --- | --- | --- |
| `planning/product/index.md`, `01`–`13-*.md` | Current product identity, module manifest, and thirteen logical sections | Validated planner application |
| `planning/features/CHG-*/specification.md` | Feature delta, lifecycle, accepted intent, implementation references | Validated planner application |
| `planning/open-items.md` | Actionable uncertainty with stable `CLR-` IDs | Validated item serializer |
| `planning/tasks/<batch>/` | Immutable approved task stories and scoped `contract.json` | Task generator |
| `.planner/project.json` | Stable repository IDs, roles, remote identities | Planning root |
| `.planner/config.md` | Category owners and optional identity fallback | F-17 settings / planner |
| `.planner/mcp.json` | Harness MCP advertisement | F-18 editor |
| `planning/imports/` | Reference documents | Operator import |
| `planning/archive/specification-pre-modules.md` | Pre-migration single-file evidence | Migration only |
| `$PACKET_HOME/projects/<slug>/` | Private chat, checkpoints, checkout mapping | Local Packet process |
| Git common directory | Transaction journal, queue, implementation records/worktrees metadata | Packet workflow |

The legacy `planning/specification.md` is migrated and removed from the current path (D-17 superseded by D-28). `SPECIFICATION.md` at the repository root remains historical scaffolding; its removal is deferred. D-23/D-25 establish this repository's current owner, not a universal default.

### 7.2 Core types

`OpenItem` carries `id`, kind, category, priority, authority (`Agent`, `Review`, `Human`), owner, question, reason, optional `feature_id`, recommendation, evidence, and open/resolved status. Existing items default conservatively to Human authority. `TaskBatchRef` and `BatchContract` freeze the approved feature, affected product-module contents, repository base commits, and configuration; every story names one target repository. `Implementation` records target worktree, verification, PR/auto-publish state, and merged commit.

### 7.3 Structured protocol

Schema v2 `TurnEnvelope` uses `assistant_message`, `document_updates[{document_id,content}]`, open-item additions/updates/resolutions, `next_question_id`, and workflow/task fields when applicable. `document_id` is an application allowlist (`product:index`, `product:<module>`, `feature:CHG-nnn`); agents cannot provide filesystem paths. Each changed document is complete. Validation precedes transactional apply. Schema v1 `updated_specification` remains accepted only for unmigrated legacy projects and MUST NOT mix with modular updates (D-05 superseded by D-29).

### 7.4 Private and derived state

Local checkout paths are private and are verified against `.planner/project.json` remote identity before use. Conversation, activity, and generation checkpoints are working memory. Repository surveys and any future full-text/vector indexes are derived and disposable; a semantic match must be reopened at its source before a durable decision. Conversation may expire after its accepted knowledge is written to authoritative artifacts (D-07, ratified by D-31).

### 7.5 Configuration

The connected git identity precedes `.planner/config.md` identity fallback (FR-13, D-14). Category ownership is person, group, or seat-inherited. `PACKET_HOME` changes private storage; `PACKET_PI_BIN` and `PACKET_TURN_TIMEOUT_SECS` affect harness discovery and deadlines. The project manifest holds no machine-specific paths. No collaboration wire payload is current (D-18, D-22; D-32 rules the channel's trust ground and topology, and D-33 rules its writer model as true concurrent co-authoring). When the post-MVP channel lands, D-31 bounds its wire payload class to planning artifacts and presence - specification, open items, presence; chat never crosses the wire, and $PACKET_HOME owes no shared/private split.
