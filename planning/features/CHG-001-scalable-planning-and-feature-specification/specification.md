# CHG-001: Scalable Planning and Feature Specifications

**Status:** Implemented

**Implementation:** `f72e3ae`, `77b3131`, `c020290`, `7239bb8`, `2b14482`, `6011b25`, `ace60b2`, `5bb9c4e`, and `32168c4`; evidence index: `docs/chg-001-acceptance-evidence.md`

**Origin:** Operator proposal, 2026-09-14

**Authority:** Accepted change contract, reconciled into `planning/product/`.

**Affected repositories:** `packet` (this repository). Multi-repository support is part of the desired behavior; no additional concrete repository is currently registered.

## Intent

Make Packet useful for long-lived brownfield products without putting the whole product history into every model request. Preserve git-backed product truth, concise feature deltas, context chosen for the activity, and a Kanban board that shows actionable work. Packet should investigate and resolve safe uncertainty itself, reserving chat questions for blocking human decisions.

The durable authority order is: accepted product and feature artifacts; direct repository evidence for observed behavior; private working state; disposable derived indexes; model recollection. A completed feature's lasting behavior moves into the product specification. Git retains its historical feature specification and task evidence.

## Current Behavior

- `planning/specification.md` is one physical Markdown file. `src/core/state.rs::PlannerState` loads and mirrors it as `spec_text`; `src/core/context_build.rs::TurnContext::build` places the whole text in every normal planning context. `src/core/prompt.rs` asks for a full replacement, and `src/core/validation.rs` validates its single `updated_specification` field. `src/core/apply.rs` writes that file before checkpointing. The 13-section layout gate exists in `src/core/specification.rs`, but it applies to one document.
- The current file on this branch has accumulated long revision narratives and feature implementation diaries. It has also been rewritten after the recent coherent-specification policy commit (`57194b0`), so the existing prompt and structure gate have not kept it concise. This is an observation about current files, not a ruling that the long form is intended.
- `src/core/workflow.rs` binds readiness to the exact monolithic specification text. `src/core/task_generation.rs` uses that text in its generation identity and private checkpoints; `src/artifacts/task_docs.rs` freezes it into task batches. Existing batches under `planning/tasks/` are historical snapshots and must remain valid.
- `src/domain/item.rs::OpenItem` has kind, priority, category, assignee, question, reason and status; it has no authority classification or feature reference. `planning/open-items.md` is the git-backed queue. `src/core/routing.rs` checks seat eligibility; current workflow readiness treats any unresolved Blocking item as a stop, regardless of whether Packet could investigate it autonomously.
- `src/ui/layout.rs` already combines planning items and implementation tasks on a Kanban board. The current board has five implementation-derived columns and item/task detail views. `src/app/root.rs` exposes readiness and implementation controls. `src/core/implementation_queue.rs::Queue` defaults Auto on. A new-feature approval gate before Auto begins is not represented in that queue state.
- `src/core/repo_overview.rs` surveys one connected repository. `src/core/turn.rs` runs Pi in that repository. The implementation runner uses per-task worktrees within one git repository; tasks have no target repository ID. `src/persistence/` stores private chat and checkpoints; no `.planner/project.json` repository manifest exists.
- The Pi harness is already an external process boundary (`src/harness/`). The change can retain it.

## Desired Behavior

### Product and feature authority

`planning/product/index.md` and the thirteen independently replaceable Markdown modules under `planning/product/` together form one current product specification. `index.md` contains product identity, status, a short summary, the module manifest and active feature references. The module names are `01-vision.md`, `02-scope.md`, `03-actors-and-roles.md`, `04-feature-inventory.md`, `05-functional-requirements.md`, `06-non-functional-requirements.md`, `07-data-model.md`, `08-architecture.md`, `09-environment.md`, `10-decisions.md`, `11-risks.md`, `12-acceptance.md`, and `13-source-map.md`. The UI may render them continuously.

Every material capability or behavior change receives `planning/features/F<number>-<slug>/specification.md`, using stable, never-reused change IDs. Its required sections are Intent, Current Behavior, Desired Behavior, Scope, Affected Product Areas, Requirements, Decisions and Assumptions, and Acceptance Criteria. It describes the delta, not unrelated product material. Status moves through Draft, Ready, Implementing, Reconciliation, Implemented, or Abandoned. The approved feature is a delta over the current product; unimplemented behavior must not appear as current product truth. Completion reconciles the actual merged behavior into only affected product modules, then marks the feature Implemented with implementation references. Material divergence creates a review or human decision item instead of silently ratifying code.

New features use compact stable IDs in their folder names, such as `planning/features/F10-add-id-to-features/specification.md`. Every generated task carries its parent feature ID in both its filename and heading, such as `F10-TASK-generate-id.md`. Existing `CHG-nnn` features and numbered task files remain valid historical artifacts and are not renamed.

The specification view opens on the Product Specification and can switch to a selected document from the set of concurrently active Features. Task stories live on the Kanban rather than in a duplicate document tab. The Kanban board remains the primary oversight surface and a projection of validated workflow/items, never a second store of accepted facts.

### Context and memory

Authoritative project memory lives in tracked product, feature, item, task, project and configuration artifacts. Repositories are directly inspected evidence. Private working memory may hold conversation, active feature, inspected files, temporary hypotheses and resumable checkpoints; it cannot become accepted truth by itself. Derived summaries, symbol/full-text indexes and optional future embeddings must be disposable and rebuildable with source references. No accepted requirement, decision, assumption or criterion may exist only in a cache. Durable conclusions from conversation are promoted to project artifacts; old conversation can leave model context without losing those conclusions.

An application-controlled Context Builder compiles four layers for each invocation: standing policy/contract; small project orientation (`product/index.md`, repository map, seat/ownership); working feature/items/recent conversation/task; and on-demand retrieved modules, source, tests, history or other repositories. Always include the active feature and relevant open items, but select product modules deterministically by explicit IDs, affected areas, repository IDs and dependencies. Retrieve further evidence by explicit reference, structured metadata, repository/file search, full-text search, then optional semantic discovery. Semantic matches must be resolved to source artifacts before durable decisions. Normal planning prompts must remain bounded as history grows; merely existing material is not a reason to load it.

Planning, task generation, implementation and reconciliation receive different context. Task batches freeze the approved feature, product-module references, repository base revisions and planning configuration identity. They do not require the entire product specification. Implementation workers receive their task and relevant approved contracts; reconciliation receives approved intent, actual merged code and affected modules. The external Pi harness remains unchanged and can read additional authoritative files on demand.

### Planning root and repositories

The connected repository is automatically the planning root for a single-repository project. A multi-repository project records stable logical repository IDs, roles and repository identity in `.planner/project.json`; machine-specific checkout paths remain private. Project-level planning artifacts exist only in the planning root. Other repositories are evidence and implementation targets. Every feature identifies affected repositories; every task targets exactly one repository, with cross-repository dependencies explicit. Packet does not promise atomic commits or publication across repositories.

### Uncertainty, conversation and oversight

Open items retain kind, category, priority and owner and add a separate authority: Agent, Review or Human. Existing items migrate conservatively to Human unless a safe classification can be proved without changing behavior. Repository-observable facts and reversible details normally belong to Agent; moderate assumptions to Review; product behavior, scope, external contracts, acceptance, destructive migration, security, privacy, compliance and material operations to Human. A human promotion to Human must never be silently downgraded.

Before asking, Packet inspects specifications, repository evidence and earlier decisions and considers safe inference or a reversible assumption. It asks at most one chat question only for an unresolved **Human / Blocking** item eligible under current routing. Agent items can be investigated without interrupting the user; nonblocking Human and provisional Review items stay actionable on the board. A recommended answer should accompany a necessary question. Normal conversation should usually be under approximately 120 words and state what changed, what Packet concluded and what is blocked; detailed activity belongs in item/worker views.

The board must distinguish equivalent states of To Do, Active, Needs Review, Blocked / Needs Attention and Done, and visually separate agent work, provisional review, human decisions, blockers, implementation tasks and reconciliation. Card type, authority and priority are independent dimensions. It shows actionable items only; routine retrieval, temporary hypotheses, settled decisions and historical completed features do not create cards. A card's detail exposes evidence, relevant feature, owner, priority, authority and recommendation where applicable. Lightweight Approve, Reject, Edit/correct, Defer, Assign and Open related spec actions should operate through validated artifact mutations. Nonblocking review must be actionable without a chat interruption.

Packet may autonomously investigate, create/update feature drafts, classify affected areas, resolve Agent items, propose Review decisions and determine readiness. **Explicit human approval is required before implementing a newly planned feature.** Preserve the existing task-generation approval boundary and offer it through the UI. Auto mode may continue already-approved queued work but cannot bypass this new-feature gate.

### Document update contract and migration

The planning envelope replaces the single specification replacement with `document_updates: [{document_id, content}]`. Logical IDs identify approved product modules or feature documents; only the application maps IDs to allowed repository paths. Arbitrary model-supplied writable paths are forbidden. A changed module is returned in full; unaffected modules are neither returned nor rewritten. Document and open-item mutations pass one all-or-nothing validation gate and durable git checkpoint. The board derives from accepted state.

On first use, migrate `planning/specification.md` by parsing the thirteen sections into matching modules, creating `index.md`, retaining content and stable `G`, `F`, `FR`, `NFR`, `D` and `CLR` identifiers, and preserving git history. Migration must be restart-safe and idempotent, protect against existing target conflicts and symlink/path escapes, and avoid losing state on partial failure. Existing task batch snapshots, item IDs, implementation state and Kanban cards remain. Existing chat may stay private; only uncaptured durable conclusions need promotion. Migration does not reinterpret historical task snapshots as current product truth.

## Scope

In scope: modular product storage and migration; feature documents/lifecycle; logical document updates and validation; scoped context/retrieval; planning root and repository manifests; repository-specific tasks/dependencies; authority-aware open items and routing; approval gates; board/specification UI; reconciliation and private/derived memory boundaries. All model interaction continues through Pi.

Out of scope: vector database or mandatory embeddings, a new authoritative database, replacing Kanban or Pi, direct provider APIs, cross-repository atomic transactions, autonomous new-feature implementation without approval, rewriting historical feature/task specs, permanent full-conversation model context, and changes to the deferred real-time collaboration feature.

The change does not weaken the currently accepted MVP definition of done in the product specification. Its own acceptance criteria below govern this feature's delivery. Product module 12 retains the existing bar until a separate explicit decision changes it.

## Affected Product Areas

| Area | Product modules / current IDs | Repository evidence |
| --- | --- | --- |
| Product authority and history | `01`, `02`, `04`, `10`, `12`, `13`; existing IDs retained | `planning/specification.md`, `docs/living-specification-policy.md`, `src/core/specification.rs` |
| Actors, ownership, uncertainty | `03`, `05`, `06`, `07`, `11`; `D-14` routing remains | `src/domain/item.rs`, `src/core/routing.rs`, `src/core/validation.rs`, `src/artifacts/items_io.rs` |
| Storage and architecture | `07`, `08`, `09`; current artifact/turn requirements remain unless explicitly superseded | `src/core/state.rs`, `src/core/context_build.rs`, `src/core/turn.rs`, `src/core/apply.rs`, `src/artifacts/` |
| Task and delivery contract | `04`, `05`, `07`, `08`, `12` | `src/core/workflow.rs`, `src/core/task_generation.rs`, `src/artifacts/task_docs.rs`, `src/core/implementation.rs`, `src/core/implementation_queue.rs` |
| Board and document UI | `04`, `05`, `06`, `08` | `src/ui/layout.rs`, `src/ui/spec_viewer.rs`, `src/app/root.rs`, `src/app/session.rs` |
| New repository manifest | `02`, `07`, `08`, `09` | `.planner/project.json` (to be added); current single-repository root behavior in `src/core/state.rs` |

No existing product ID is renumbered or silently redefined. Reconciliation will record explicit supersessions where this feature changes a prior ruling (for example, single-file product authority and question policy).

## Requirements

| ID | Requirement |
| --- | --- |
| `CHG-001-R1` | Packet MUST maintain one logical, human-readable current product specification across the listed independently replaceable modules and a concise index. |
| `CHG-001-R2` | Every material change MUST have a concise feature delta with lifecycle, affected areas/repositories and its own acceptance criteria; current product truth MUST remain unchanged until reconciliation. |
| `CHG-001-R3` | Every model-authored document mutation MUST use an allowlisted logical ID and full replacement of that document. Validation MUST be all-or-nothing across documents, items and workflow before writes/commit. |
| `CHG-001-R4` | Migration MUST preserve stable IDs, current content, git history, existing task snapshots, queue and board state; it MUST be idempotent and safe across interruption. |
| `CHG-001-R5` | Context MUST be activity-specific, bounded and selected from authoritative sources by deterministic references first. Repository inspection and optional semantic retrieval MUST resolve to source evidence before durable decisions. |
| `CHG-001-R6` | Project artifacts MUST live in a planning root. Multi-repository projects MUST use stable repository IDs with private checkout paths; each task MUST target exactly one repository and explicit dependencies. |
| `CHG-001-R7` | Open items MUST separate authority from priority, preserve human promotions, and remain actionable on the board until resolved. Chat questions MUST be limited to one eligible Human / Blocking item after autonomous investigation. |
| `CHG-001-R8` | A newly planned feature MUST require explicit human approval before implementation, including Auto queue execution. Straightforward approval and review actions SHOULD be available on the board. |
| `CHG-001-R9` | After merged implementation, reconciliation MUST compare approved intent and code, update only affected product modules, and mark the feature Implemented only when discrepancies are settled. |
| `CHG-001-R10` | Product, feature, task and activity views MUST remain understandable; the board MUST project actionable workflow rather than become a competing store of truth. |
| `CHG-001-R11` | Authoritative knowledge MUST survive expiration of chat context and deletion/rebuild of derived indexes. Derived summaries MUST retain source references. |
| `CHG-001-R12` | Existing Pi harness and resumable task generation/verification behavior SHOULD remain; task batches MUST freeze scoped approved contracts and repository revisions. |

## Decisions and Assumptions

- **Confirmed from the proposal:** Modular product authority; per-feature deltas; git as historical and authoritative project storage; bounded layered context; deterministic retrieval preference; optional/disposable semantic indexing; explicit implementation approval; single-target tasks; board as oversight projection; deferred peer collaboration unchanged.
- **Observed, not accepted intent:** Current workflow uses exact monolithic specification text as a readiness fingerprint, and Auto defaults on. The migration must replace the fingerprint with a scoped approved feature/product revision identity and enforce approval before Auto dispatch.
- **Implementation choice, reversible:** Use `CHG-001` as the first change identifier because no `CHG-` feature artifacts were found in this checkout. The slug is descriptive and not part of the stable ID. Add a repository-local allocator that prevents reuse across completed/abandoned features.
- **Implementation choice, reversible:** Treat this feature document as Ready because the proposal gives a testable scope and acceptance bar and repository inspection established the current seams. Ready does **not** grant implementation approval.
- **Review during implementation:** Define repository identity and local checkout mapping format without committing machine paths. Reject ambiguous or unavailable repositories before generating/starting tasks; do not guess a target. Preserve the existing single-repository path when no manifest exists.
- **Review during implementation:** The current per-file rename writes are not a multi-file transaction. The new logical-update contract requires an application-level staged transaction/recovery journal or equivalent mechanism so failure between files does not expose a partially accepted document set. This is a design obligation, not a claim about current behavior.


### CLR-025 — Approved provisional decision

> Approve: add a mirror-of-005 gate ticket to that task's batch, scoped to f773b2d's footprint (src/ plus tests/multi_repository_feature.rs plus the CHG-001 spec delta) on the pinned Rust 1.98 toolchain; treat f773b2d as pre-existing debt for CHG-005 purposes only.

Evidence considered:

> f773b2d = +885/−281 over 25 files; 18 src paths outside CHG-005's seven-path set; never captured by any clippy baseline or D-34 pass; CHG-005 gate record /tmp/d34_gate_df5e3d9/RECORD.md foreign-commit dossier; operator ruling in ticket 005 conversation: 'the other local changes are from another task, ignore those'.

## Acceptance Criteria

The feature is complete only when all of the following are demonstrated with concrete artifacts and tests. The numbered list preserves the operator's acceptance bar.

1. A large brownfield product with many implemented features does not inject all historical feature specs into a normal planning turn.
2. A new feature receives a concise delta specification in its own `CHG-` directory.
3. Planning loads and changes only relevant product modules; unaffected module bytes and git paths remain untouched.
4. Agent-authority uncertainty is resolved from evidence without asking the user.
5. A nonblocking Human item appears on the board without interrupting chat.
6. At most one question is asked, only for an eligible blocking Human decision.
7. Task generation uses approved feature intent and scoped product/repository context.
8. Completed implementation reconciles lasting behavior into affected product modules.
9. A feature spanning multiple repositories identifies them and generates single-repository dependent tasks.
10. A single-repository project migrates without losing planning IDs or historical task artifacts.
11. The modular product specification remains readable as one logical rendered document.
12. Document/item/workflow mutations retain all-or-nothing validation and git-backed durability, including interruption recovery.
13. Conversation can expire from model context without loss of accepted knowledge.
14. Planning, task generation, implementation and reconciliation receive distinct activity-appropriate contexts.
15. All derived summaries/indexes can be deleted and rebuilt without loss of authoritative project knowledge.
16. Normal prompt size stays bounded as repositories, tasks and feature history grow; demonstrate with growing fixtures rather than an invented token threshold.
17. Deterministic IDs/references are preferred when sufficient, without invoking semantic retrieval.
18. Any optional semantic/vector match is checked against its underlying authoritative source before durable use.
19. The Kanban board remains available after migration with preexisting actionable cards.
20. The board distinguishes Agent, Review, Human, blocking, implementation and completed states.
21. Non-actionable observations, settled decisions and temporary hypotheses do not produce cards.
22. A nonblocking provisional decision can be reviewed and resolved directly on the board.
23. Board state is reconstructable from validated workflow/items without an independent authoritative board store.

Evidence must include migration/restart fixtures, bounded-context and multi-repository scenarios, rejected-document zero-mutation proofs, approval/Auto gate tests, reconciliation mismatch handling, a full regression run, and direct desktop interaction with the board and document switcher. Passing a build or one scripted fixture alone does not establish completion.
