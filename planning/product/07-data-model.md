## 7. Data Model

### 7.1 Artifact layout (inside the connected repository)

| Path | Content | Written by |
| --- | --- | --- |
| `planning/specification.md` | Complete current specification | Agent (accepted turns) |
| `planning/open-items.md` | Queues serialized behind a generated-by banner | App (domain to Markdown) |
| `planning/imports/` | Imported reference documents, optional .md twins | Operator via app |
| `.planner/config.md` | Category-to-owner map; optional current-operator override block | F-17 dialog / editor |
| `.planner/mcp.json` | MCP servers advertised to the session | F-18 editor (atomic write; absent or blank ⇒ unconfigured) |
| `SPECIFICATION.md` (root) | Provisional MVP contract seed — superseded and slated for removal per D-17; its intent is absorbed in this document; full text remains retrievable from git history | Legacy human artifact; not reproduced |

### 7.2 Open Item

Fields: `id` (fixed CLR prefix, hyphen, three decimals); `priority` in {blocking, high, normal} rendered as BLOCKING/HIGH/NORMAL badges; `kind` in {question, ambiguity, assumption, ownership}; `category` (free-form string routed against config, extensible per project); `assigned_to` (person, group, category, or unassigned); `question`; `reason` (why it matters); `status` in {open, resolved}. Resolving an item removes it; provenance survives in git history. Parsing edges tolerate loose capitalization and common aliases emitted by the harness.

### 7.3 Turn Envelope (schema v1)

JSON object: schema_version, assistant_message, change_summary (short imperative phrase feeding the commit subject), updated_specification (complete replacement, null means unchanged), open_items_added (list), open_items_updated (patches by id, null fields untouched), open_items_resolved (ids), next_question_id (must satisfy routing rules). Field names accept camelCase or snake_case at the parse edge; strictness applies downstream at validation.

### 7.4 Operator state (outside the repository)

Root at `$PACKET_HOME` overriding `~/.packet`; per project under `projects/<slug>/`. Slug = sanitized repository basename plus a 16-hex FNV-1a fingerprint of the canonical path; deterministic. Holds the chat log; deliberately not shared (decision D-07; its boundary versus the D-18 collaboration channel is open under CLR-017, and the channel itself is post-MVP per D-22).

### 7.5 Configuration

Category-to-owner map (persons or groups) plus an optional current-operator override block (name and group memberships); projects may add further categories (Contract §7). Per D-14 the operator's *primary* identity is the git user of the connected repository (FR-13; per D-23 the chair's seat is **Zachary Barno** ⟨zbarno@gmail.cfg⟩… correction: ⟨zbarno@gmail.com⟩), so the override block demoted from declaration to fallback/echo. Persisted as Markdown with a tolerant parser and canonical serializer (F-17);
semantic: an entry names a role's holder — a person (sole-owned) or a group (shared); an absent entry means the role is seat-inherited. (v1.2 state: no absent entries — every non-General category names the chair as sole holder, D-25.)

### 7.6 Wire payloads (prospective)

None at baseline; none at the MVP close, since the D-18 channel is post-MVP (D-22). Whatever crosses the channel when it arrives is undecided (CLR-015/016/017); the only binding constraint recorded so far is NFR-1's: the wire is transient and git checkpoints are the reconciliation authority.
