## 8. Architecture

Layers, top to down: `ui` (chat pane, spec viewer, items pane, layout, overlays, toasts, theme) → `app` (session orchestration, root frame, welcome, dialogs) → `core` (state, context_build, prompt, turn, validation, apply, ownership, routing, ids, gitops, repo_overview) → `harness` (AiHarness trait; PiHarness: process supervision, event stream, envelope extraction, live-progress sink). Cross-cutting: `artifacts` (layout constants and IO helpers, incl. `config_io`), `persistence` (home, chat store), `domain` (item, stakeholder, user, chat-log types), `error`.

Harness discovery and health (ratified as-built, D-13): binary located via `PACKET_PI_BIN` env override → `PATH` scan → common install locations (`~/.npm-global/bin`, `~/.local/bin`, `~/.pi/bin`); a ≤10 s `pi --version` probe validates it, and the parsed version is surfaced in the harness label as plain text (`pi {version}`). Version policy is deliberately display-only: no minimum-version floor and no pinning — whichever installed pi version the operator has is considered valid (an absent or dying probe is the only failure, surfacing as a `HarnessNotFound`/`HarnessFailed` diagnostic naming the searched sources and the override variable). Precision note (v1.2): `label()`'s display site is no longer owed — it feeds the landed F-16 in-app guide section (ticket 003).

Turn sequence (F-5):

1. Snapshot planner state and recent chat into immutable turn inputs.
2. Build context: curated repository survey, current spec, open items, config, system instructions (persona, routing duties, response contract).
3. Spawn pi in the repo working directory; stream events; poll the cancel flag between stream events.
4. Extract the candidate envelope from final text.
5. Validate strictly (ids, enums, references, routing). Failure means reject: nothing is mutated, problems are reported.
6. Apply the normalized changes: spec replacement, item adds/updates/resolves, ownership-gap synthesis.
7. Atomic-write every touched planning path.
8. Stage only those paths; commit as Packet Planner with the turn's change-summary subject.
9. Report outcome: Applied (adopt state, expose commit SHA or git error), Rejected (problems, final text), or HarnessFailed (typed error, stderr tail).

Normative behaviors carried from the Contract:

- **Routing (D-14 law):** an item is poseable to the seated operator iff it is (a) categorized General, (b) assigned to a group the operator belongs to, (c) in a category explicitly owned by the operator personally, or (d) in a category with no explicit owner at all (seat inheritance). Items sole-owned by another particular user stay visible in the panel but are never posed; the app enforces eligibility against the agent's `next_question_id` at validation. Selection prefers Blocking, ties broken by smallest item number. (Fully encoded and enforced since ticket 002 / PR #2, v1.2 verified.)
- **Ownership guard (§8):** categories without owners generate Ownership items so nothing can orphan on paper; D-14 seat inheritance ensures nothing orphans in practice. Idle in this repository since D-25 left no unowned lane.
- **Rollback discipline (§16):** validity gates all mutation; an invalid turn costs nothing.
- **Investigate first (§18, §22):** check the repository before asking a human; favor progress over exhaustive interrogation.

Landed deltas versus the v0.1.0 baseline — ALL SHIPPED, verified read-only in the v1.2 pass against master (`fb54229`):

- **Ticket 001 / PR #1 (`10f8e36`) — FR-13 identity.** `state.rs::effective_user` is git-first (`gitops::read_config(user.name → user.email)`, then the config Current-User block, then (guest)); re-derived on resync descent; `IdentitySource` domain type records the winning source; settings dialog echoes with override-only semantics; `root.rs` projects the display cache; ticket-text guard and advisory `run.lock` in `core/implementation.rs` intact. The v0.9 worktree-era observations (complete-but-unmerged diff) are SUPERSEDED — that exact work merged as PR #1.
- **Ticket 002 / PR #2 (`a31bc84`) — D-14 routing law.** `routing.rs::evaluate(item, user, stakes: &Stakeholders)` decides all four branches (sole ownership granted to its owner, group sharing, seat inheritance for a named chair and never for (guest), General broadcast) with the regression matrix in-tree (spot-verified: `sole_owner_is_poseable_on_their_own_lane`, `unowned_lane_is_seat_inherited_by_a_named_chair_but_never_guest`), enforced at envelope validation against `next_question_id`.
- **Law-text propagation.** The as-built three-rule summary is replaced by the D-14 law in the prompt/context build, including a per-seat lane digest (the rendered source of the 'Sole-owned lanes: …' line in the operator context — spot-verified in `prompt.rs`; it is this digest that proved the running build recognizes the D-25 sole-ownership lane for the seated chair).
- **Ticket 003 / PR #3 (`c375705`) — F-16 guide section.** The 'Set up the pi harness' compose-and-paint spots are live in `DlgSettings` (`src/app/dialogs.rs`).
- **Ticket 004 / PR #4 (`feedbbd`) — F-18 MCP editor.** `DlgMcp`-family card in `dialogs.rs`, header wiring in `root.rs`; syntax-probe save and blank-removal per D-16.
- **Tickets 005/006 — demonstration readiness.** Fixture preparer and scripted runbook landed (`examples/prepare_exit_demo.rs`, `docs/exit-demo-runbook.md` — correction: `docs/exit-demo-runbook.md`); the NFR-8 invariant-leg certification ran under 006. No code deltas owed by either.

Owed code delta versus baseline: NONE for the MVP feature set (above). Newly directed and OWED as of v1.4 (both now unblocked — F-20's typology locked by the operator's CLR-020 answer in v1.4; each awaits its story, slotted behind ticket 007): the F-19 graph relocation/restyling and the F-20 card type color-coding — pure UI deltas in `src/ui/`, with no core/harness/data-model changes anticipated (F-20 owes none specifically: the card class is already structurally known at paint time).

**Reserved territory (D-18; phasing ruled post-MVP by D-22; design gated on CLRs 015–017, non-normative until the post-MVP channel project begins).** No collaboration machinery exists at baseline, and three collision sites are identified ahead of design: (i) *Transport* — the dependency graph carries no socket or async crate (audited: anyhow, chrono, egui/eframe, pulldown-cmark, serde/serde_json only), so the channel arrives with a runtime decision against NFR-7's offline/minimal posture — a decision now safely scheduled outside the MVP; (ii) *Projection* — which slice of planning state (and whether chat logs) serializes to the wire, with D-07's chat-locality as the boundary question (CLR-017); (iii) *Concurrency* — the single-worker guarantee in `core/turn.rs` and the bare stage/commit wrappers in `core/gitops.rs` (no index-lock detection) presume one session per checkout; two peers on a shared working tree would contend on git's own locks, and Google-Docs-grade co-authoring would require a merge/CRDT layer present nowhere in the architecture (CLR-016).
