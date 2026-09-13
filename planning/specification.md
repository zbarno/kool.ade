# Packet — Living Technical Specification

**Version:** 1.3 · **Status:** Implementation in progress; MVP exit evidence not yet certified here.

**Authority:** Single standing authority for product intent and requirements (`D-17`).

**Origin:** Human-authored root `SPECIFICATION.md`, subsequent operator decisions, and repository observations.

**Latest revision:** Adopt the coherent specification policy; consolidate audit history; distinguish current delivery/UI scope from the original MVP; correct implementation-base freshness and private activity storage. Preserve the accepted exit criteria.

**Maintenance.** The planning agent proposes complete revisions after accepted planning turns; Packet validates and persists them with git checkpoints. Operators normally steer through chat; the UI does not offer direct specification editing. Explicitly authorized repository maintenance may revise this file. Stable identifiers and superseded decisions remain in the document; git preserves detailed history.

## 1. Vision

The central hypothesis is that a continuously driven external AI harness can turn a loosely defined idea into a useful, continuously maintained specification by inspecting the repository, identifying uncertainty, routing questions, and recording answers.

- `G-1` Conversation produces a specification consistent with accepted intent.
- `G-2` Uncertainty becomes typed, prioritized, routed open items rather than disappearing silently.
- `G-3` Git provides durable shared state, diffs, rollback, and historical provenance.
- `G-4` Packet orchestrates work through an external harness seam; the harness performs model interaction.

## 2. Scope

### In scope

Single-repository desktop planning, repository inspection, interview chat, read-only specification rendering, routed open items, validation-gated persistence, git checkpoints, document import, MCP configuration advertisement, local chat history, settings, and manual Pi onboarding.

Current operator-approved delivery scope also includes task generation, isolated implementation worktrees, verification and recovery, PR tracking, default-on Auto mode, and sequential queue advancement. Planning items and implementation tasks share a Kanban board. These extensions supersede the original seed's exclusions of task generation and autonomous coding (`D-26`); they do not change the historical MVP exit bar in §12.

### Out of scope

Embedded inference, direct provider API integration, a separate project database, PM-tool integrations, complex authorization, in-app specification editing, and automatic harness installation. No standalone ADR/risk registry or analytics dashboard is required.

### Platform and deferred work

The launch target is the operator's Linux x86_64 workstation, built and run from source (`D-11`). Other platforms, architectures, and packaged distribution are excluded from this launch commitment.

WebSocket collaboration is explicitly post-MVP (`D-18`, `D-22`); topology, trust, writers, and shared content remain unresolved. Additional harness backends and a real-team pilot are deferred. The channel is the first designated collaboration feature after MVP; sequencing against other deferred work requires a decision when that horizon opens.

## 3. Actors and Roles

### General ownership model

| Actor | Responsibility / authority |
| --- | --- |
| Operator | Defines intent, answers eligible questions, configures ownership, starts or resumes delivery |
| Category owner / group | Answers questions for its assigned lane under `D-14` |
| Main planning agent | Maintains the specification, investigates uncertainty, communicates progress and manages planning/delivery context |
| Implementation worker | Executes a task in its dedicated worktree, reports task activity and acceptance evidence |
| Packet application | Validates responses, assigns CLR IDs, persists artifacts, owns commits and delivery transitions |
| External Pi harness | Runs model sessions and repository inspection through its configured environment |

`D-14` is the routing authority: General broadcasts; direct personal/group address and explicitly owned lanes are eligible; a git-identified seat inherits unowned lanes. A lane claimed by other holders vetoes direct address. Ownership items are handled through configuration, never posed as chat questions. Other owners' items remain visible.

### Current assignments

The checked-in `.planner/config.md` names Zachary Barno as the sole owner of Product, Development, QA, InfoSec, UX, and Operations (`D-25`). It also lists that name under General, whose effective routing remains broadcast. No groups are configured. These assignments are project state, not a restriction on other projects' ownership models.

## 4. Feature Inventory

Statuses below describe source-backed implementation, not certification of the live MVP demonstration.

| ID | Capability | Status | References |
| --- | --- | --- | --- |
| `F-1` | Connect a git repository; survey manifests, README and pruned tree | implemented | `FR-9` |
| `F-2` | Interview chat with one active planning turn and cancellation | implemented | `FR-5` |
| `F-3` | Read-only Markdown specification and streamed preview on a white paper surface | implemented | `FR-1`, `D-27` |
| `F-4` | Planning open items in the shared Kanban board with detail modals | implemented | `FR-6`, `D-27` |
| `F-5` | Snapshot, context, harness, validation, apply, atomic writes and checkpoint pipeline | implemented | §8 |
| `F-6` | Structured schema-v1 response gate | implemented | §7.3 |
| `F-7` | Typed, prioritized open-item lifecycle with stable application-minted IDs | implemented | `FR-7`, `FR-8` |
| `F-8` | User-scoped routing and validation-time enforcement | implemented | `D-14` |
| `F-9` | Planning-path git checkpoints and repository status display | implemented | `FR-3`, `FR-11` |
| `F-10` | Sanitized document imports and optional readable Markdown twins | partial | IO exists; interactive import validation remains owed |
| `F-11` | Raw MCP configuration advertisement to Pi | implemented | `D-16`, §7.5 |
| `F-12` | Manager progress and separate task activity, including available usage telemetry | implemented | `D-27`, §7.4 |
| `F-13` | Welcome and first-run guidance | implemented | `src/app/welcome.rs` |
| `F-14` | Operator-local chat persistence | implemented | `D-07` |
| `F-15` | Configurable fixed-at-start planning deadline | implemented | `D-24` |
| `F-16` | In-app manual Pi setup guide and discovery status | implemented | `D-12`, `D-13`, `D-15` |
| `F-17` | Identity/settings and category ownership editor, including existing-owner selection | implemented | `D-14`, `D-25`, `D-27` |
| `F-18` | Raw JSON MCP editor with nonblocking syntax warning | implemented | `D-16` |
| `F-19` | Readiness-gated task generation and immutable batch specification snapshots | implemented | `src/core/workflow.rs`, `src/core/task_generation.rs` |
| `F-20` | Resumable worktree execution, verification, PR tracking and Auto queue | partial | Mechanisms exist; comprehensive self-healing is not proven, §11 |
| `F-21` | Coherent living-specification authoring policy and structural replacement validation | implemented | `D-28`, `FR-18` |

The deferred collaboration channel receives a feature identifier when its project is defined, consistent with `D-18`.

## 5. Functional Requirements

- `FR-1` The specification MUST contain the complete current document, never a delta or conversation transcript.
- `FR-2` Normal application specification changes MUST originate from accepted agent turns. Direct UI editing MUST NOT be offered. Explicit repository maintenance follows the Maintenance statement.
- `FR-3` An accepted mutating planning turn MUST produce one checkpoint with a short imperative change-summary subject. A non-mutating turn MUST commit nothing; commit failure follows `FR-11`.
- `FR-4` Validation failure MUST cause zero artifact mutation and no commit. Prior artifacts MUST remain byte-identical and problems MUST be surfaced.
- `FR-5` Cancellation MUST terminate the active turn promptly and discard unaccepted streamed fragments. Artifact writes MUST follow `NFR-2`.
- `FR-6` The agent MUST pose only questions eligible under `D-14`; invalid selections MUST reject the turn. Ineligible items MUST remain visible on the board.
- `FR-7` A non-General open-item category lacking an owner MUST have a synthesized Ownership item. Seat inheritance makes the lane answerable pending nomination.
- `FR-8` CLR IDs MUST be unique, application-minted and monotonically allocated; retired numbers MUST NOT be reused.
- `FR-9` Planning Pi sessions MUST run in the connected repository and inspect available evidence before asking humans questions that the repository can answer.
- `FR-10` Imported reference documents MUST remain in the repository and available to Pi.
- `FR-11` Adopted in-memory state MUST match written artifacts even if the following commit fails. Packet MUST surface that failure.
- `FR-12` Silence MUST NOT shorten a turn's deadline. The timeout is fixed at turn start; invalid overrides fall back to `D-24`.
- `FR-13` Identity MUST derive from repository `user.name`, then `user.email`, then configured Current User, then `(guest)`. Connection and settings-save MUST rederive it.
- `FR-14` New implementation worktrees MUST use a freshly fetched base without overwriting unrelated checkout work. Resume MUST preserve existing work and reject incompatible task/worktree identity.
- `FR-15` Recoverable execution failures SHOULD retry automatically. Recurrent preventable failures MUST be diagnosed and corrected within authorized scope, with durable recovery evidence and atomic commits. A retry limit MUST NOT be treated as successful completion.
- `FR-16` Auto mode MUST default on, integrate verified task work without requiring a PR, and advance to the next eligible queued task after successful integration. PR mode MUST retain periodic current-state reconciliation.
- `FR-17` The Kanban board MUST be the primary work view. Clicking an item MUST open its properties and activity in a large bounded modal; task output MUST have an expandable activity view. Main chat MUST carry the project manager's user communication; worker activity belongs to the task. Main and task specification surfaces MUST use a white paper background.
- `FR-18` Material accepted information MUST produce a coherent full revision under `docs/living-specification-policy.md`: stable IDs, explicit decision supersession, distinct intent/evidence/uncertainty, current statuses, concise revision notes, and the ordered 13-section layout. The application MUST reject replacement documents with an invalid title or section structure before persistence. Semantic truth remains an authoring and review obligation.

## 6. Non-Functional Requirements

- `NFR-1` Git MUST remain the durable shared planning store. Operator chat and private delivery runtime records MUST remain outside tracked project artifacts (§7.4). Future peer transport MUST NOT replace git as reconciliation authority.
- `NFR-2` Planning artifacts MUST use temporary-file-plus-rename writes. This is per-file atomicity; a multi-file turn is not a filesystem transaction.
- `NFR-3` Packet MUST refuse concurrent planning submissions within one application instance. Dedicated implementation worktrees and advisory locks isolate delivery. Cross-process co-authoring of the same planning checkout is not supported; its future model is unresolved.
- `NFR-4` Worker execution MUST remain decoupled from UI updates, with progress and Cancel available during long runs. No quantitative latency, throughput or usability threshold is binding (`D-19`).
- `NFR-5` Model/MCP networking MUST remain delegated to Pi configuration. Remote delivery uses external git/gh and operator authentication (`D-26`). Packet MUST NOT add accounts or telemetry. Git operations use argument arrays; imported basenames are sanitized. Collaboration trust remains deferred.
- `NFR-6` `AiHarness` MUST isolate provider-specific process behavior. Pi is the current required adapter; other adapters and installation automation remain deferred.
- `NFR-7` The source build targets Linux x86_64 with Rust edition 2024, thin-LTO release builds and a minimal dependency posture. Offline Cargo operation requires cached dependencies; it does not imply offline model or git service availability.
- `NFR-8` The full regression suite MUST pass and the build MUST be warning-free. Relevant regressions MUST have meaningful tests. Test counts are evidence for a revision, not a fixed acceptance target.
- `NFR-9` Git history MUST use short imperative checkpoint subjects, without per-message commit churn. Detailed historical evidence belongs in git and retained run artifacts.

## 7. Data Model

### 7.1 Artifact / Storage Layout

| Path / Store | Content | Writer / Owner |
| --- | --- | --- |
| `planning/specification.md` | Current authoritative specification | Planning agent through validated application writes |
| `planning/open-items.md` | Current unresolved items | Application serializer |
| `planning/imports/` | Reference documents and optional Markdown twins | Operator through import flow |
| `.planner/config.md` | Current-user fallback/groups and category owners | Settings dialog |
| `.planner/mcp.json` | Raw external MCP configuration | MCP editor |
| `.planner/workflow.json` | Interview brief, reviewed specification, task-batch references | Planning workflow |
| `planning/tasks/` | Generated stories, batch index and frozen specification snapshots | Task-generation workflow |
| Git common directory: `packet-implementations/` | Per-ticket execution state, reports and recovery/activity evidence | Implementation runner |
| Git common directory: `packet-queue.json` / `packet-queue.lock` | Auto preference, queue state and execution lock | Queue coordinator |
| Sibling `.packet-worktrees/` | Isolated implementation and integration worktrees | Implementation runner |
| `$PACKET_HOME/projects/<slug>/` | Private conversation history | Local persistence |
| Root `SPECIFICATION.md` | Retired seed; removal remains an ungated chore | Historical artifact (`D-17`) |

### 7.2 Core Domain Types

`OpenItem` records `id`, `priority`, `kind`, `category`, `assigned_to`, `question`, `reason`, and `status`. Kinds are Question, Ambiguity, Assumption, Ownership; priorities are Blocking, High, Normal. Categories are extensible strings. Resolved items leave the active queue; history retains their provenance and allocated IDs.

`CurrentUser` and `Stakeholders` represent the seat, groups and category holders. Configuration identity is fallback state; effective identity follows `FR-13`.

`Workflow` holds an optional `InterviewBrief`, the exact `reviewed_specification`, and task-batch references. Readiness applies to that exact text. `TaskStory` describes intent, design, affected files, implementation steps, acceptance, verification, dependencies, edge cases and rollout. Scope/success references are one-based and validated for coverage.

`Implementation` binds a ticket and its saved text to a branch, base and worktree, plus status, verification/publication state and recovery metadata. Resume cannot silently replace task scope. `Queue` stores `auto_mode`, `running`, `current_ticket`, and `last_error`; its default is Auto enabled but not running.

### 7.3 Structured Protocols / Envelopes

The planning `TurnEnvelope` uses schema v1. A nonblank `assistant_message` is required by validation. `schema_version` is optional at the parser boundary; if present, it MUST equal 1. `updated_specification` is a complete replacement or null; changed replacements must satisfy §5's layout gate. `change_summary` supplies the checkpoint subject.

`open_items_added`, `open_items_updated`, and `open_items_resolved` default to empty when omitted. Updates reference existing IDs; null patch fields leave values unchanged. `next_question_id` is optional and routing-validated. `interview`, `task_outline`, and `task_stories` extend the workflow and are validated for their turn purpose. Snake-case and camelCase field aliases are accepted. Invalid IDs, enums, references, routing or document structure reject the whole turn before writes.

Implementation reports carry `status`, `summary`, criterion-specific `acceptance_criteria` evidence, runnable `verification` commands, and `remaining` work. Completion requires matching ticket criteria, passing checks and no unresolved work. A process exit code alone is insufficient evidence.

### 7.4 Local or Private State

Private chat and resumable task-generation checkpoints use `$PACKET_HOME`, default `~/.packet`. Project slugs combine sanitized repository basename with a canonical-path FNV fingerprint. Chat is not tracked in git (`D-07`).

Task execution/activity evidence and queue metadata live in the git common directory, shared by the local repository's worktrees but outside tracked content. Live planning previews are provisional and not accepted artifacts. Task output may be retained for recovery and expanded activity inspection; do not describe all activity as transient.

Future sharing of conversation or activity requires resolving `CLR-017`.

### 7.5 Configuration

`.planner/config.md` is Markdown with a tolerant parser and canonical serializer. It stores category holder lists and a Current User name/groups block. Identity precedence is authoritative in `FR-13`; an absent owner permits seat inheritance, subject to `D-14`.

Nonblank `.planner/mcp.json` is appended as raw prompt context, capped at 4,096 characters. Packet does not broker MCP or validate a server schema. The editor warns on malformed JSON but permits saving; blank save removes the file. See `D-16`.

Runtime environment overrides are listed in §9. Missing queue metadata uses the defaults in §7.2; malformed persisted queue JSON is an error, not a silent reset. Planning-format migration beyond tolerant parsing is not yet defined.

### 7.6 Reserved peer protocols

No collaboration wire protocol is implemented. Topology, transport payloads, compatibility and writer coordination remain future design, bounded by `D-18`, `D-22` and `CLR-015`–`CLR-017`.

## 8. Architecture

Current hierarchy: `ui → app → core → harness`, with shared `domain`, `artifacts`, and `persistence` modules.

- `ui`: board, chat, specification, activity, layout and modal rendering.
- `app`: session state, dialogs, main-agent communication and queue coordination.
- `core`: planning, routing, validation/application, task generation, git and implementation orchestration.
- `harness`: `AiHarness` boundary; Pi discovery, process/event handling, extraction and progress.
- `artifacts` / `persistence`: shared Markdown/JSON artifacts and private local storage.

### Planning flow

1. Snapshot repository state and recent conversation.
2. Assemble repository survey, current specification, open items, configuration and standing authoring/routing/workflow instructions.
3. Run Pi in the connected repository and stream provisional progress.
4. Extract the response envelope and validate it, including changed specification structure.
5. Apply normalized changes and synthesize required ownership gaps.
6. Atomically write each changed planning artifact.
7. Stage only touched planning paths and create one checkpoint.
8. Adopt written state and report the commit result, validation rejection, or harness failure.

The application owns writes; the planning model is instructed to inspect read-only. Validation precedes every artifact write, but it cannot mechanically prove semantic truth or unchanged acceptance intent.

### Implementation flow

1. Select an eligible task; validate its immutable scope and acquire execution ownership.
2. For new work, fetch an explicit `origin` base into a private ref. Auto uses the remote default branch. PR mode compares local/remote ancestry and refuses divergence rather than discarding work.
3. Create a dedicated worktree, or resume its preserved state after identity checks.
4. Run the worker, retain activity and recovery evidence, validate its completion report, and execute reported verification commands.
5. Repair recoverable failures within the configured recovery policy; preserve unresolved work and surface the stopping reason.
6. Publish through PR mode, or integrate verified work through an isolated Auto integration worktree with an atomic commit and safe remote update.
7. Reconcile PR state periodically. After successful Auto integration, advance the queue subject to dependencies and existing PRs.

Verification commands run in fresh POSIX shells rooted at the task worktree, with `PACKET_WORKTREE` available for stable absolute paths. Shell variables and directory changes do not carry across commands.

### Current limitations and future architecture

Per-file atomic writes do not make multi-file application transactional. Planning concurrency is per instance; queue/implementation locks do not establish a multi-client planning protocol. Automatic retries and correction mechanisms exist, but universal self-healing and arbitrary failure repair are not established guarantees (`F-20`).

The peer channel is reserved future architecture; no listener, relay, CRDT or shared-chat implementation is implied by its direction decision.

## 9. Environment, Launch, and Preconditions

### Hard prerequisites

- Linux x86_64 graphical workstation, local Rust toolchain supporting the manifest and installed git.
- A writable git repository and sufficient filesystem space for private state and sibling worktrees.
- Operator-installed Pi CLI and its required model/service configuration.
- For remote implementation: an accessible `origin` and working external git authentication. PR mode additionally requires configured `gh`; Auto does not require a PR.
- Offline Cargo commands require locally cached dependencies.

### Configuration and launch

| Setting / command | Meaning |
| --- | --- |
| `PACKET_PI_BIN` | Explicit Pi executable; otherwise search `PATH`, then `~/.npm-global/bin`, `~/.local/bin`, `~/.pi/bin` |
| `PACKET_TURN_TIMEOUT_SECS` | Positive integer seconds read at turn start; invalid, zero or overflow falls back to twelve hours (`D-24`) |
| `PACKET_HOME` | Override private chat storage root |
| `PACKET_WORKTREE` | Runner-provided absolute task worktree for verification commands |
| `cargo run --offline` | Build and launch from source with cached dependencies |
| `cargo test --offline` | Full automated regression suite |

Pi availability uses a bounded ten-second `--version` probe; version is displayed without a pin or minimum floor (`D-13`). In-app settings provide the setup guide.

### Operational caveats

Missing git identity falls back per `FR-13`; a guest does not acquire git-identified seat privileges. Provider access and repository credentials are supplied externally. Long silent model runs retain their configured deadline. No quantitative performance promise or packaged deployment is implied.

## 10. Decisions Log

| ID | Decision | Basis | Status |
| --- | --- | --- | --- |
| `D-01` | Root `SPECIFICATION.md` governs MVP intent | Original human contract | Superseded by `D-17` |
| `D-02` | Use a Rust desktop UI on eframe/egui 0.36.1 | Manifest and UI source | Observed / needs ratification |
| `D-03` | Invoke system git and author planning checkpoints as Packet Planner, `planner@packet.local` | `src/core/gitops.rs` | Observed / needs ratification |
| `D-04` | Use external Pi, without embedded inference or direct provider calls | Original contract | Confirmed |
| `D-05` | Use the structured schema-v1 planning envelope | Existing contract and parser | Provisional; core contract largely confirmed, compatibility behavior observed |
| `D-06` | Application mints fixed-prefix CLR IDs without reusing gaps | ID allocation implementation | Observed / needs ratification |
| `D-07` | Keep interview history operator-local and outside git | Local persistence implementation | Observed / needs ratification; future sharing open in `CLR-017` |
| `D-08` | Default turn budget is two hours, with override fixed at turn start | Earlier README/implementation | Superseded by `D-24`; override mechanics retained |
| `D-09` | Distinguish stream silence from death and surface stderr diagnostics | Process implementation and regression tests | Observed / needs ratification |
| `D-10` | Use Rust edition 2024 and thin-LTO releases | Cargo manifest | Observed / needs ratification |
| `D-11` | Launch on the operator's Linux x86_64 workstation from source; no packaging obligation | Operator decision | Confirmed |
| `D-12` | Provide manual harness setup guidance; require only Pi now, other adapters later | Operator decision | Confirmed |
| `D-13` | Discover Pi by override, PATH, common install directories and bounded version probe; display version without pinning | Operator ratification of implementation | Confirmed |
| `D-14` | Use git-first identity and exclusive/group/seat-inherited category routing, configured through in-app settings | Operator decision; routing/state/config source | Confirmed |
| `D-15` | Deliver the manual Pi guide in settings, with discovery status; no per-repository editable guide or standalone document owed | Operator decision | Confirmed |
| `D-16` | Provide an in-app raw JSON MCP editor; syntax warnings do not block saving, blank removes configuration, saves are atomic and checkpointed | Operator decision | Confirmed |
| `D-17` | Make `planning/specification.md` the sole standing authority; retire the root seed and remove it eventually without a deadline | Operator decision | Confirmed; supersedes `D-01` |
| `D-18` | Retain git durability and add a future transient WebSocket collaboration channel; no accounts/telemetry and model/MCP traffic remains external | Operator decision | Confirmed direction; original exclusive-network-reservation premise superseded by `D-26` |
| `D-19` | Exclude quantitative performance/usability thresholds from the current MVP acceptance bar | Operator decision | Confirmed; new thresholds require a fresh ruling |
| `D-20` | Require the seven-outcome live fixture demonstration plus full regression suite; exclude real-team pilot and standalone automated walkthrough from the gate | Planner proposal under operator delegation | Confirmed by `D-21` |
| `D-21` | Ratify `D-20` without amendment as the binding MVP definition of done | Operator confirmation | Confirmed |
| `D-22` | Defer WebSocket collaboration and its two-participant walkthrough until post-MVP; retain design concerns as nonblocking prerequisites | Operator decision | Confirmed phasing; original zero-network premise superseded by `D-26` |
| `D-23` | Operate under the repository's git identity; defer explicit category nominations | Operator decision | Confirmed identity; nomination deferral completed by `D-25` |
| `D-24` | Set default turn budget to twelve hours; retain fixed-at-start positive-seconds override and invalid-value fallback | Operator ratification of observed value | Confirmed; supersedes `D-08` |
| `D-25` | Assign Zachary Barno sole ownership of Product, Development, QA, InfoSec, UX and Operations | Operator settings action; `.planner/config.md`, `fb54229` | Confirmed |
| `D-26` | Include task generation and recoverable worktree delivery; refresh remote bases, reconcile PRs, default Auto on and advance after verified integration without a required PR | Operator implementation/recovery requests; current runner and queue source | Confirmed; supersedes seed coding exclusions and `D-18`/`D-22` exclusive-network premises; resolves `CLR-019` |
| `D-27` | Use a shared Kanban for planning and tasks, large item/activity modals, white specification surfaces, selectable existing owners, and proactive main-agent communication separate from worker activity | Operator UI and project-manager requests | Confirmed |
| `D-28` | Maintain the coherent 13-section Living Technical Specification under the authoring policy; preserve IDs, decision supersessions and accepted acceptance criteria | Operator's specification-format/content instruction | Confirmed |

## 11. Risks and Open Concerns

### Active concerns

| Item / reference | Concern | Wake condition |
| --- | --- | --- |
| `CLR-015` | Peer topology, discovery, transport trust and authentication are undecided | Begin post-MVP channel design |
| `CLR-016` | Single author versus concurrent co-authoring and git reconciliation are undecided | Before peer writing is implemented |
| `CLR-017` | Shared artifacts versus private interview/activity content is undecided | Before any collaboration payload is defined |
| `F-20`, `FR-15` | Bounded recovery exists; comprehensive prevention and healing of recurring implementation failures still needs demonstrated coverage | Recurrent or new execution failure |
| `F-10` | Import IO exists; interactive import behavior still needs direct validation | Import dogfood |
| §12 | A full live seven-outcome demonstration must be evidenced independently of build/test success | MVP exit review; inspect ticket 007 evidence |

### Recently resolved

`CLR-019` is resolved by the operator's remote-refresh request (`D-26`) and the implemented explicit fetch/base selection. The obsolete local-HEAD-only observation is not current behavior. Ownership nominations and timeout drift are explained by `D-25` and `D-24`; their old discussions need not remain here.

### Accepted debt and deferred validation

The retired root seed remains pending removal (`D-17`). Shared artifact migration beyond tolerant parsing has no defined strategy. Multi-process planning writes lack coordinated ownership; git locks alone are not a collaboration design. Deferred channel, alternate adapter and pilot sequencing needs a fresh decision when activated. Unratified observations remain marked in §10.

The layout gate cannot prove factual accuracy, stable semantic identity, or preserved acceptance meaning. Those obligations require evidence-aware authoring and review; structural success MUST NOT be presented as semantic certification.

## 12. Acceptance / Definition of Done

Success scenario: the operator installs Pi using the in-app guide, connects a repository under the derived identity, describes a feature, and receives a repository-informed specification. Categorized questions route to the eligible seat; answers become decisions and clear items while checkpoints preserve the result.

**Binding MVP bar:** both legs below MUST pass (`D-20`, `D-21`). This revision changes presentation and current-scope documentation, not that bar. Only a fresh explicit operator decision may change it.

### Behavioral / demonstration evidence

Use a small prepared self-contained git fixture with naive planning state and a fixed runbook. Exercise a real model through a live multi-turn interview. Required outcomes MUST be observed in-session and legible in resulting git history:

1. The specification improves using repository insight: meaningful sections, corrected false claims and concrete code references.
2. Items arise in at least two distinct categories with sensible assignments.
3. Ownership-gap synthesis fires for at least one unowned category. Seed that gap in the fixture; Packet's own current categories are all assigned.
4. At least one answer becomes a recorded decision and clears its item, with the specification updated.
5. Checkpoints form an intact chain with imperative subjects.
6. A deliberately invalid or misrouted envelope causes zero mutation: prior artifacts remain byte-identical and problems are surfaced.
7. Cancellation of an in-flight turn discards unaccepted fragments cleanly.

`examples/prepare_exit_demo.rs` and `docs/exit-demo-runbook.md` are the preparation and execution vehicles. Preserve fixture state, git history and observed outcome evidence. A scripted/stubbed model test is not a substitute for this live demonstration.

### Automated / invariant evidence

The FULL regression suite MUST pass with a warning-free build (`NFR-8`), including routing, validation/no-mutation, settings/guide and MCP regressions. Retain commands and results tied to the evaluated revision. A historical test count or worker assertion alone does not certify this leg.

### Exclusions and deferred validation

Quantitative performance/usability thresholds are excluded (`D-19`). The timeboxed real-team pilot is post-MVP dogfood, not an exit gate. A standalone automated walkthrough is optional; mechanical invariants belong in the suite. This acceptance scenario is single-actor. Two-participant behavior and its privacy/concurrency criteria belong to the deferred channel's own acceptance definition.

## 13. Source Map

| Evidence | Location / use |
| --- | --- |
| Current intent and authoring contract | This document; `docs/living-specification-policy.md`; operator decisions recorded in §10 |
| Original seed and historical rulings | Root `SPECIFICATION.md` and git history; superseded authority under `D-17` |
| Manifest and launch guidance | `Cargo.toml`, `README.md` |
| Planning instructions and pipeline | `src/core/prompt.rs`, `src/core/turn.rs`, `src/core/validation.rs`, `src/core/specification.rs`, `src/core/apply.rs` |
| Ownership and identity | `src/core/routing.rs`, `src/core/ownership.rs`, `src/core/state.rs`, `.planner/config.md` |
| Task generation and snapshots | `src/core/workflow.rs`, `src/core/task_generation.rs`, `src/artifacts/task_docs.rs`, `planning/tasks/` |
| Execution and queue | `src/core/implementation.rs`, `src/core/implementation_queue.rs`, private git-common-directory evidence |
| Harness and process lifecycle | `src/harness/harness.rs`, `src/harness/pi_harness.rs`, `src/harness/pi_proc.rs`, `src/harness/pi_extract.rs` |
| UI and main-agent coordination | `src/app/root.rs`, `src/app/dialogs.rs`, `src/app/manager.rs`, `src/ui/spec_viewer.rs`, `src/ui/task_activity.rs`, `src/ui/items_pane.rs` |
| Artifact/configuration/private storage | `src/artifacts/`, `src/persistence/`, `.planner/workflow.json` |
| Automated evidence | Unit tests beside source; `tests/task_workflow.rs` and `tests/fixtures/` |
| Live MVP acceptance vehicle | `examples/prepare_exit_demo.rs`, `docs/exit-demo-runbook.md`, ticket 007 under `planning/tasks/packet-mvp-git-native-desktop-specification-planner/` |
| Historical identity/ownership delivery | Git commits `10f8e36`, `a31bc84`, `fb54229`; retained as locating references, not current validation results |
