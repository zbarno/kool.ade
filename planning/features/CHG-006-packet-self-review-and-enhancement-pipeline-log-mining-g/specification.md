# CHG-006: Packet Self-Review and Enhancement Pipeline — Log Mining, Generated Ideas, and Approvable Enhancement Cards

**Status:** Draft
**Directed by:** Operator Main Chat request: Packet must review itself to find areas of improvement (new features, quality of life, bug fixes); it must stay aware of itself even when the project it is working on is not Packet; it should review its own logs and the project files it is working on; the LLM brainstorms and ideates ideas (generative and novel); ideas are reviewed and evaluated to identify the best, which is added to the ToDo as a new type "Enhancement"; the kanban item carries an approval button in addition to normal steering/conversation inputs; once approved it is broken down into tasks and implementation begins.
**Bound:** A self-improvement loop for the Packet project only. The §12 MVP exit bar (D-20/D-21) is untouched; routing/veto (D-14), the explicit-approval gate (D-29, FR-14), and the deferred channel (D-18, D-22, D-31 artifacts-only) are untouched.

## Intent

Packet's own improvement opportunities — quality-of-life friction, latent bugs, missed feature openings — surface today only by operator accident, and even less when the operator is sitting Packet while driving *another* project. The operator wants a deliberate self-improvement loop: Packet reviews (a) its own logs — turns, rejections, conversations, checkpoints — and (b) the planning files of the project currently in front of it, uses the LLM to brainstorm *generative, novel* improvement candidates (new features, quality of life, bug fixes), evaluates them to identify the best one, and puts that winner on its own board as a first-class **Enhancement** ToDo card carrying an **Approve** button. On approval the card is converted into a proper CHG feature, broken down into an ordered task batch, and handed to the standard implementation queue — all of it reachable from any project's seat, since Packet improves itself while the operator is elsewhere.

Intended user: the seated operator (Zachary Barno, D-23), who increasingly works other projects in Packet and would lose track of Packet's own rough edges. Beneficiary of every change is every project operated through Packet. Explicitly not wanted: self-modification without approval, sweeping other projects, or touching anything except Packet itself.

## Current Behavior

Code-grounded, read-only audit of master:

- The board has five card classes: Question, Ambiguity, Assumption, Ownership (ItemKind, src/domain/item.rs) plus task cards from task batches (class palette and legend per D-27/CHG-002, src/ui/theme.rs, src/ui/task_activity.rs). No "Enhancement" class exists anywhere (grep-confirmed).
- Approval gestures already exist on the board in two places: "Approve provisional decision" for Review items (src/ui/task_chat.rs:452 → src/core/board_actions.rs::approve_review) and "Approve feature for implementation" on a Ready feature card (src/ui/layout.rs:195 → src/core/workflow.rs::approve_feature, which snapshots the feature contract hash into .planner/workflow.json and commits). Task generation refuses to run without that record (src/core/workflow.rs::prepare) — the D-29/FR-14 gate.
- All LLM activity runs through external Pi (D-04) as planning / investigation / reconciliation turns launched inside the active project's repository by the AiHarness (src/harness/*). No "analytical ideation turn" mode exists, and nothing today targets a repository other than the active project's.
- The reviewable trace comprises: the active project's planning artifacts (planning/open-items.md, planning/resolved-items.json, planning/tasks/<batch>/ stories and index, git checkpoint history authored by 'Packet Planner'), plus operator-local stores under $PACKET_HOME (per-project chat history and task-conversations.json, src/persistence/*). Turn rejections and failures already surface in chat with machine-readable reasons (see this project's own conversation history), which makes a rich friction/defect corpus.
- Cross-repository *work* support exists — ProjectManifest registers repositories and each story pins a target_repository (F-23) — but planning context compilation, interviews, and approval operate on the planning root only.

## Desired Behavior

**R1 — The self-review run.** A new operator-invoked run (cadence per CLR-023; at minimum a manual "Run self-review" action) launches a special Pi turn that is read-only over every repository. Its compiled context package carries: Packet's own planning artifacts and checkpoint history; a size-bounded sample of the operator-local $PACKET_HOME logs from the projects driven (chat/task conversations); and the source of Packet's own repository, resolved at run time (placement per AS-1). The turn returns a dated **review bundle**: observed problems and friction (each with an evidence pointer into the corpus) and a brainstorm of candidate improvements spanning new features, quality-of-life polish, and bug fixes. Ideation is explicitly prompted to be *generative and novel*; every candidate is checked against the current product modules, shipped/active CHGs, and open items so nothing duplicates live or shipped capability or contradicts recorded intent.

**R2 — Evaluation and promotion.** The candidates are assessed (same or follow-on structured pass) on operator impact, alignment with vision and modules, feasibility against current architecture, and duplication risk. The ranking with reasons is shown to the operator (chat or card view). Exactly ONE best candidate is promoted (AS-3); runners-up stay visible in the bundle.

**R3 — The Enhancement card.** Promotion mints a new board card of the new class **Enhancement** (legend grows a sixth entry) into the ToDo column, carrying: title, problem statement with evidence from the bundle, anticipated effect, and the originating review id. The card has the normal steering/conversation inputs PLUS an **Approve** button. Decline is expressed through the normal card conversation and recorded in the bundle with the operator's stated reason.

**R4 — Approval → spec → tasks → queue.** Approve on the card executes one atomic, validated turn against the **Packet repository**, even when the active project is something else: mint the next CHG id, write a feature document seeded from the card (intent/current/desired behavior/scope distilled from card plus bundle; normative sections shaped for immediate task generation), record the explicit implementation approval for that id in workflow.json using the existing contract-hash machinery (semantics per the CLR-024 ruling), generate the ordered task-story batch frozen to that contract, and feed the batch to the standard Auto queue (verify-and-merge per story). The operator watches the card move to Implementing with the batch's task cards beneath it.

**R5 — Guardrails.** Collection is strictly read-only; only the approval path writes (feature doc, workflow record, task batch, git checkpoints) and only to the Packet repository. No new crates; offline posture preserved (NFR-7); envelope/validation invariants untouched; the review run obeys the existing 12-hour budget (D-24) and cancellation (FR-5); D-34's bar applies to this change itself (regressions green, no new clippy warnings versus the pinned Rust 1.98 baseline). Runs guard against racing a live Auto batch or an in-flight planning turn.

## Scope

In:

1. Run trigger and orchestration: the manual action (placement top bar vs Settings, settled in task design), the cadence hook(s) per CLR-023, and busy-state guards (no run while a planning turn is in flight or an Auto batch is mid-story, with a visible reason).
2. Review-context compiler: the bounded corpus from Packet's planning artifacts and checkpoint history, bounded sampling of operator-local $PACKET_HOME logs (privacy-neutral: local-only, no new sharing surface), and Packet repository source refs; location resolution per AS-1.
3. A special Pi "review/ideation" turn type with its own validated envelope: structured findings plus a candidate pool; deterministic novelty/dedup checking (id/title scan) against product modules, the feature directory, and open items, complemented by the LLM's semantic judgement.
4. The evaluation pass and ranking presentation; single-winner promotion.
5. The Enhancement card class: domain kind, legend/color entry, ToDo spawning, Approve button plus normal conversation inputs, decline path with reasoned retirement.
6. The cross-repository approval pipeline: CHG mint, feature seeding, approval recording, story generation, Auto-queue handoff — validated and atomic like any other turn (FR-2/FR-4 lineage).
7. Persistence: review bundles and declined candidates private under $PACKET_HOME (D-07 lineage); git-backed artifacts only through the standard pipeline.
8. Tests: corpus bounding, envelope validation, dedup logic, approval atomicity, cross-repository separation with the active project on a non-Packet repo; D-34 regression bar.

Out:

1. Improving any repository other than Packet (projects are improved only from their own seats);
2. implementing unapproved ideas, bypassing the verify/merge pipeline, or promoting multiple enhancements at once;
3. distributing enhancement cards or review bundles over the deferred channel (D-18/D-22; D-31's artifacts-only surface unchanged);
4. telemetry, cloud ideation, new crates, or semantic/vector index services;
5. retrospective re-reviews of concluded features beyond what their git history already holds.

## Affected Product Areas

Proposed behavior only until merged-code reconciliation (FR-15, D-28):

- **Module 02 (Scope)** — `product:02-scope`: a self-improvement capability sentence at reconciliation; the "autonomous implementation of unapproved new features" exclusion stands (approval is the human gesture).
- **Module 04 (Feature Inventory)** — `product:04-feature-inventory`: new F-26 row at reconciliation.
- **Module 05 (Functional Requirements)** — `product:05-functional-requirements`: new FRs for the review-run duty (bounded corpus, read-only), the Enhancement class and its Approve duty (extends FR-19's board-approval vocabulary), and the cross-repo approval pipeline's atomicity.
- **Module 07 (Data Model)** — `product:07-data-model`: ItemKind gains Enhancement (parsers backward-compatible), the review-bundle record (private store), reuse of the existing approval record.
- **Module 08 (Architecture)** — `product:08-architecture`: the self-review subsystem stages (context compile → review turn → evaluation → promotion → approval pipeline) and foreign-repository resolution for harness turns.
- **Module 10 (Decisions Log)** — `product:10-decisions`: D-entries at the rulings' approval (cadence, approval depth).
- **Module 12 (Acceptance / Definition of Done)** — `product:12-acceptance`: an additive bar mirroring the ACs.
- **Module 13 (Source Map)** — `product:13-source-map`: rows at reconciliation.
- **Code (anticipated, proposed paths):** src/domain/item.rs (kind), src/ui/theme.rs + src/ui/task_activity.rs (palette/legend), src/core/board_actions.rs (Enhancement approval), src/core/workflow.rs (approval-recording reuse), new src/core/self_review.rs (orchestrator/compiler/evaluator), src/harness/* (turn cwd/repository parametrization), src/app/* (trigger + card button), src/persistence/* (bundle IO under $PACKET_HOME), prompt tails for the review turn. No Cargo.toml dependency changes.

## Requirements

- **REQ-S1-1 (MUST).** A self-review run compiles a bounded, read-only corpus (Packet planning artifacts + checkpoint history; operator-local $PACKET_HOME logs per sampling policy; Packet source per AS-1) and yields a validated, dated review bundle of findings and candidates; no repository file is modified during collection.
- **REQ-S1-2 (MUST).** Every candidate carries an evidence pointer into the bundle and a novelty verdict computed against the current product modules, shipped/active features, and open items; duplicates or intent-conflicts are demoted with the colliding module/CHG cited.
- **REQ-S2-1 (MUST).** The bundle presents candidates ranked with reasons; exactly one winner is promoted per run (AS-3).
- **REQ-S3-1 (MUST).** The winner appears as an Enhancement-class ToDo card carrying problem statement, evidence, anticipated effect, and origin review id; the class legend gains the entry; the card offers normal steering/conversation inputs PLUS an Approve button.
- **REQ-S4-1 (MUST).** Approval atomically mints the next CHG id, writes its feature document, records the explicit implementation approval (semantics per the CLR-024 ruling), generates the ordered story batch frozen to that contract, and feeds it to the Auto queue — correctly executed with any other project active; a validation failure leaves no partial state (FR-4 lineage).
- **REQ-S4-2 (MUST).** Declining from the card conversation retires the card with the stated reason recorded in the bundle and mints no CHG.
- **REQ-ALL-1 (MUST).** The D-34 bar holds (regressions green, no new clippy warnings versus the pinned baseline), no new crates, and NFR-7's offline-launch posture is preserved.
- Cadence-dependent requirements (automated trigger hooks, if ruled) are (PENDING CLR-023); approval-depth mechanism details are (PENDING CLR-024).

## Decisions and Assumptions

- **DE-1 (open, CLR-023):** self-review cadence — manual-only versus manual-plus-automatic-hooks. Blocking for Scope item 1's final form.
- **DE-2 (open, CLR-024):** approval depth — the card's Approve acting as the single formal D-29 implementation approval (recommended), versus a second conventional "Approve feature for implementation" step afterward.
- **AS-1 (agent, provisional):** the Packet repository's location is an operator-configured setting (precedent: F-10 import placement), validated at run start; missing/invalid path fails the run with a clear diagnostic. Reversible; final placement settled in task design.
- **AS-2 (agent, provisional):** review bundles and declined candidates persist privately under $PACKET_HOME (D-07 lineage); only the winner and its downstream artifacts enter git.
- **AS-3 (agent, provisional):** one Enhancement card per run, per the operator's "identify the best idea" phrasing; runners-up remain browsable in the bundle.
- **AS-4 (agent, provisional):** dedup combines the deterministic id/title scan with LLM semantic judgement over modules 01–13 and the feature directory, biased conservatively toward demoting borderline matches with reasons recorded.

## Acceptance Criteria

1. A self-review run with the active project set to a non-Packet repository completes, returning a validated bundle (at least one finding; at least three candidates, each with an evidence pointer and a novelty verdict), and modifies no repository files outside the private store.
2. The winner appears as an Enhancement ToDo card exposing an Approve button alongside its normal conversation inputs; declining from the card retires it with the reason recorded in the bundle and no CHG minted.
3. Approve on an Enhancement card, with the active project a non-Packet repository, produces in one validated turn: the next CHG id, its seeded feature document, the workflow.json approval record, the frozen ordered story batch in the Packet repository's planning/tasks, and Auto-queue admission; interrupting mid-turn leaves every prior byte intact.
4. A candidate duplicating a shipped or in-flight capability is visibly demoted in the bundle with the conflicting module or CHG cited.
5. Busy guards: a run attempted while an Auto batch is mid-story or a planning turn is in flight is refused or deferred with a visible reason, and the review run never starves the implementation queue.
6. Full regression suite green; zero new clippy warnings versus the pinned Rust 1.98 baseline; no Cargo.toml dependency changes.
