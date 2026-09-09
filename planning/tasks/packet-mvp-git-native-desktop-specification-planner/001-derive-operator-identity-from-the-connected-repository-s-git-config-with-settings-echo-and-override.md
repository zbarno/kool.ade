# 001 — Derive operator identity from the connected repository's git config with settings echo and override

Feature: Packet MVP — git-native desktop specification planner

Status: Individually validated; see batch index for generation status.

## Problem this ticket solves and why

Verified in src/core/state.rs:108-112, PlannerState::effective_user consults only the .planner/config.md Current User block and then falls back to ("(guest)"); it never reads the connected repository's git config. Consequently no live session can ever seat at the D-23 chair (Zachary Barno, verified in the connected tree's git config), seat inheritance of unowned lanes can never accrue to any real operator, and the DlgSettings identity fields behave as naked declarations written to config instead of echoing the identity actually in force. Until this delta lands, FR-13 does not exist at runtime, and the later D-14 routing-law ticket would enforce eligibility for a phantom (guest) user, defeating the exit-bar demonstration's single-actor seating.

## Ticket goal — what changes when done

Before: connecting to a git-identified working tree still resolves identity from the config block alone or '(guest)', and the settings dialog shows whatever the config declares. After: PlannerState.load resolves the seated operator per FR-13 (git user.name, then user.email, then the config Current User block, then '(guest)') at connect and on every resync (including after settings-Save); the settings dialog echoes that seated identity with a visible provenance line and the fields act as the override tier. Completion is observable: cargo test --offline is green with the new precedence tests, and opening Settings in the connected tree shows 'Zachary Barno' labeled as derived from git user.name. Actual posing of lane-bound questions remains ticket 2; this ticket makes the seat real.

## User story

As the operator running Packet from source against a working tree that carries a git identity, I want the planner to seat me as that git user and show the derivation plainly in the settings dialog, so that unowned planning lanes accrue to my seat and the config identity block is understood as a fallback override rather than the declaration.

## Purpose

The as-built effective_user in core/state.rs jumps straight from the config block to (guest) without ever reading the connected repository's git config, so no live session can seat at the D-23 chair (Zachary Barno, verified in git config), seat inheritance never accrues, and the settings identity fields remain naked declarations. This ticket lands the FR-13 git-identity delta in planner state and re-purposes the DlgSettings fields into echo-plus-override so unowned lanes can finally seat-inherit to the chair.

## Specification references

Source: [Approved specification](specification.md)

## Implementation context

Current behavior (verified): PlannerState::load (src/core/state.rs) reads spec/items/config from disk and builds no identity; effective_user() clones config.user or fabricates '(guest)'. Consumers: core/turn.rs:169 (TurnInputs user), core/context_build.rs:65 (TurnContext.user fed to the prompt), and validation/workflow/apply tests that load plain temp dirs (no .git). The UI's display cache root.rs derive_caches calls session::routing_user (src/app/session.rs:85), which falls back to the PROJECT NAME — a third, divergent identity — used by panel highlighting. DlgSettings::from_project seeds user_name/user_groups from cfg.user only; apply() writes config.md via config_io::serialize, resyncs, and commits 'settings: update stakeholders and identity'. config_io parses 'Name:'/'Groups:' under '## Current User' and nulls an unset user on re-parse. gitops::run dispatches `git -C <cwd> <args>` with argument arrays (NFR-5) and maps launch failure to AppError::Git; existing tests hand-roll temp git repos (gitops.rs mkrepo sets local user.name/user.email and commits one file). Precedent for process-global test env manipulation: src/persistence/chat_store.rs:107-120 locks ENV_HOME_LOCK while setting PACKET_HOME. Assumptions (flagged, unverified): the operator host's git honors GIT_CONFIG_GLOBAL/GIT_CONFIG_SYSTEM redirection (git >= 2.32, standard on any current Linux distro), and system git sits on PATH per section 9 preconditions. No code path constructs PlannerState as a struct literal (all sites go through load/resync — grep-verified), so adding a field cannot break constructors.

## Technical design and contracts

- New read-only probe in src/core/gitops.rs: pub fn read_config(cwd: &Path, key: &str) -> Option<String>. Implemented over the existing run() dispatcher as ['config', key] (nearest-scope-wins standard resolution, no --global/--local flags); trims output; returns None on launch error, non-zero exit, or whitespace-only output, swallowing failure rather than propagating AppError so identity probing can never block connect.
- New domain types in src/domain/user.rs: pub enum IdentitySource { GitUserName, GitUserEmail, ConfigBlock, Guest } with a pub fn label(&self) -> &'static str producing UI-ready provenance text ('derived from git user.name', 'derived from git user.email (fallback)', 'from .planner/config.md override', 'no identity found — guest'), and pub struct ResolvedIdentity { pub user: CurrentUser, pub source: IdentitySource }.
- Pure composer in src/domain/user.rs: pub fn resolve_identity(git_name: Option<&str>, git_email: Option<&str>, config_user: Option<&CurrentUser>) -> ResolvedIdentity. Precedence for NAME: trimmed-non-empty git_name beats trimmed-non-empty git_email beats config user name beats "(guest)". GROUPS always come from the config block (git has no notion of groups); when the winning name tier is git, the composed CurrentUser is { name: git value, groups: config groups or empty }. Whitespace-only values at any tier count as absent. Kept pure so the full precedence matrix is testable with zero git involvement.
- PlannerState gains one field pub identity: ResolvedIdentity, populated in load() AFTER config parses: let (gn, ge) = (gitops::read_config(repo,"user.name"), gitops::read_config(repo,"user.email")); self.identity = resolve_identity(gn.as_deref(), ge.as_deref(), self.config.user.as_ref()). effective_user() shrinks to self.identity.user.clone(), preserving its signature so turn.rs, context_build.rs, and every existing test call site compile unchanged; resync() inherits re-derivation because it reconstructs via Self::load; bootstrap_missing needs no change (it alters files, not identity inputs). Module doc on state.rs updated to state the FR-13 tier order verbatim.
- Display-cache rewiring in src/app/root.rs: derive_caches switches from session::routing_user(project.state.config.user.as_ref(), &project.state.title) to project.state.effective_user(), collapsing the third identity (project-name fallback) so panel highlighting, chat eligibility display, and turn validation share one seated user. session::routing_user (grepped: root.rs:130 is its only caller) is deleted; session.rs otherwise stays untouched.
- DlgSettings echo-plus-override in src/app/dialogs.rs: from_project seeds user_name/user_groups from proj.state.effective_user() (NOT cfg.user) and captures a new pub identity_note: String built from proj.state.identity.source.label() prefixed by the seated name; paint_settings_card renders it as a weak 11px line under 'Who am I?' (e.g. 'Seated as Zachary Barno — derived from git user.name'; when source is ConfigBlock or Guest, append 'these fields act as the override'). Save path UNCHANGED: still serializes Current User block from the edited fields, resyncs (re-resolving identity, git still beating the block), and commits 'settings: update stakeholders and identity' — storing an echoed git name in the block is harmless redundancy because the block is only consulted when git yields nothing.
- Degradation contract (section 9 soft precondition): every git identity probe failure mode (missing binary, not-a-repo, unset key, empty value, detached/weird tree) collapses to None and simply descends the tier ladder; load() never returns Err because of identity probing; a session in a git-less or identity-less tree behaves exactly as the legacy guest/config path did.

## Approved scope mapping

- Scope 4: D-14 identity and routing law: git-derived operator identity (FR-13, owed delta) and sole-ownership vs group-shared vs seat-inherited routing with enforcement at validation (F-8, owed delta), plus the in-app settings dialog carrying identity echo/override and the category-owners grid (F-17).
- Success criterion 5: The full NFR-8 regression suite is green — 106 test functions on the current tree (105 unit in src, 1 integration in tests/), including the owed D-14 routing, D-15 guide-section, and D-16 MCP-editor pins — with a warning-free build.

## Dependencies

None. This task can start independently.

## Affected files and components

- src/core/gitops.rs: add read_config(key) read-only probe over the existing argument-array run() dispatcher with failure-swallowing None contract; extend its hand-rolled temp-repo test module with fixture coverage.
- src/domain/user.rs: add IdentitySource enum, ResolvedIdentity struct, and pure resolve_identity() composer plus the doc-comment restatement of the FR-13 tier; grow the unit-test module with the precedence matrix.
- src/core/state.rs: PlannerState gains identity: ResolvedIdentity initialized in load(); effective_user() delegates to it; module header documents git-first derivation; state tests gain git-fixture repos mirroring the gitops.mkrepo pattern.
- src/app/root.rs: derive_caches routes the cached routing/display identity through project.state.effective_user(); remove the session::routing_user call so the panel and the turn share one identity.
- src/app/session.rs: delete the now-dead routing_user() helper (verified single caller in root.rs:130); Project, remember_chat, recent_chat_tuples, refresh_git, welcome_message unchanged.
- src/app/dialogs.rs: DlgSettings.from_project seeds fields from the resolved identity and a new identity_note field; paint_settings_card renders the provenance line beneath 'Who am I?'; apply() save/write/checkpoint flow intentionally untouched.

## Implementation steps

1. Add gitops::read_config in src/core/gitops.rs: build [&'static str] ['config', key] through the existing run(cwd,&[...]) helper, trim stdout, return Some(trimmed) only when exit code is 0 and the trimmed value is non-empty, else None; document that it is a read-only nearest-scope probe per NFR-5 and that failure is terminal-by-design into the tier ladder.
2. Implement resolve_identity, IdentitySource, and ResolvedIdentity in src/domain/user.rs exactly per the precedence table, treating whitespace-only as absent and taking groups exclusively from the config block; write the pure-precedence unit tests first (git-name beats config, email fallback, config-only, all-absent guest, whitespace-tier skipped) so the composer is locked before any git wiring.
3. Extend PlannerState with pub identity: ResolvedIdentity and compute it in load() immediately after config_io::parse succeeds, calling gitops::read_config twice with the already-canonicalized repo path; shrink effective_user() to clone identity.user and rewrite the stale doc comment that currently promises config-then-guest.
4. Rewire derive_caches in src/app/root.rs to project.state.effective_user() and delete session::routing_user from src/app/session.rs; compile and confirm no other references remain (rg 'routing_user' must return only zero hits outside docs).
5. Update DlgSettings in src/app/dialogs.rs: add identity_note: String; in from_project read proj.state.effective_user() for the two fields and format '{name} — {source.label()}'; insert the provenance label into paint_settings_card between the 'Who am I?' heading and the Name row; do not touch apply() write/resync/commit logic or the categories grid.
6. Write the state-level integration tests in src/core/state.rs using git-initialized temp fixtures (copy the gitops::tests mkrepo recipe): repo with local user.name plus a contradicting config block asserts git wins; unset-name fixture (shadow the ambient hierarchy — see test plan) asserts email fallback, then config, then (guest); a resync test flips local user.name on disk and asserts the new seat. Add the DlgSettings echo test by constructing a Project around the git-fixed state and asserting user_name=='Zachary Barno'-style fixture value and a note mentioning git user.name.
7. Run the full offline suite and triage any fixture that encoded the OLD config-first precedence: plain temp-dir fixtures (validation.rs base_state, workflow.rs state(tag), apply.rs) contain no .git, so read_config returns None and their identity behavior must be bit-identical — expect zero fixture edits; if one fails for that reason it means a fixture accidentally gained git ancestry and must be moved under a fresh std::env::temp_dir subdir, not papered over by weakening an assertion.

## Acceptance criteria

- Given a working tree whose local git user.name is 'Ada Lovelace' (user.email 'ada@example.org') and a .planner/config.md Current User block naming 'Bob', when the app loads the project, then PlannerState.identity is Ada Lovelace with IdentitySource::GitUserName, effective_user() returns her, and Bob's block remains byte-intact on disk.
- Given a tree with user.name unset (ambient global/system shadowed) and user.email 'eve@example.org', when loading, then the seated name is 'eve@example.org' with source GitUserEmail; if the config block also lists groups ['Ops'], then effective_user().groups == ['Ops'].
- Given a tree with NO resolvable git identity and config block Name: Dana, Groups: Ops, Platform, when loading, then the seat is Dana with groups [Ops, Platform] and source ConfigBlock; given neither git identity nor a settable config block, when loading, then the seat is '(guest)' with source Guest and load() still succeeds.
- Given the settings dialog opened on the git-identified fixture, when it is painted, then the Name field prefills 'Ada Lovelace' (the DERIVED value, not the config's 'Bob') and a provenance line states it is derived from git user.name; hitting Save without edits re-commits 'settings: update stakeholders and identity' and the dialog reopens echoing the same derived identity.
- Given a connected session, when the operator edits local git user.name to 'Mira Chen' and saves any settings change, then resync re-derives the seat and the refreshed display cache (root.rs derive_caches) reports Mira Chen with source GitUserName — proving derivation runs after settings-Save, not only at connect.
- Given the happy-path failure case of a directory that is not a git repository (plain temp dir), when PlannerState::load runs, then no error is raised from identity probing, the seat falls to config-else-guest exactly as today, and no existing unit test's identity expectations shift.

## Test plan

1. Pure composer matrix in src/domain/user.rs tests: six subcases asserting exact (name, groups, source) tuples for git-name+config, git-email+config-groups, whitespace-name-skip, config-only, all-absent-(guest), and empty-string-vs-whitespace equivalence; no filesystem or git involved.
2. Hermetic probe tests in gitops::tests: reuse the existing mkrecipe (temp dir, git init -b main, commit one file); assert read_config(repo,'user.name')==Some(fixture name) with local set; with only email configured and GIT_CONFIG_GLOBAL/SYSTEM redirected via the env lock, assert Some(email); in a plain non-git temp dir assert None for both keys without panicking.
3. Precedence integration in state::tests: build a fixture repo with local user.name 'Zachary Barno', user.email 'zbarno@gmail.com', and a seeded .planner/config.md Current User block 'Bob'; load and assert identity user name/source; rewrite the block name to 'Carol' and call resync() asserting the seat stays 'Zachary Barno'; then `git config --unset user.name` under the shadowed hierarchy and resync, asserting 'zbarno@gmail.com'/GitUserEmail, then unset email too, asserting 'Carol'/ConfigBlock.
4. Echo verification: construct Project { state: <loaded fixture state>, chat_slug: 'test-slug', .. minimal defaults } and call DlgSettings::from_project; assert user_name == 'Zachary Barno', identity_note contains 'git user.name', and from_project against the git-less temp-dir state reproduces the legacy config/guest echo with a matching source label.
5. Regression sweep: full cargo test --offline run confirming the pre-existing suite (workflow/apply/validation/plain-temp fixtures included) passes with zero fixture edits — this doubles as proof that the no-.git degradation contract preserves prior behavior bit-for-bit.

## Verification commands and expected evidence

1. Working directory /mnt/DevProj/Packet: cargo build --offline — expect compilation with zero warnings (NFR-8 warning-free bar) after the new symbols are wired.
2. Working directory /mnt/DevProj/Packet: cargo test --offline — expect the entire suite green, including the new composer-matrix, hermetic-probe, state-precedence, and dialog-echo tests described above.
3. Manual smoke (operator machine): cargo run --offline, enter the connected repository path in the connect field, click the Settings header action — expect the 'Who am I?' card to prefill Name with 'Zachary Barno' and render a provenance line referencing git user.name (the connected tree's git config is verified to carry Zachary Barno / zbarno@gmail.com per D-23 and the v0.8 pass); Save and expect a 'Saved · checkpoint <sha>' toast with the conventional settings subject in git log.

## Edge cases and failure handling

- Trigger: git present but user.name is whitespace-only (e.g. ' ') with user.email set — required behavior: the name tier is skipped as absent, email tier wins; invariant: tier descent is purely lexical, never an error.
- Trigger: ambient developer machine has a GLOBAL git identity while a fixture expects git-absence — required behavior: identity-dependent tests shadow the hierarchy by pointing GIT_CONFIG_GLOBAL and GIT_CONFIG_SYSTEM at temp control files under the ENV-home-style process lock precedent (src/persistence/chat_store.rs:107-120), holding the lock across the spawn; production read_config intentionally uses the STANDARD hierarchy (nearest scope wins), which is the D-23-desired behavior.
- Trigger: config block name differs from git name AND the operator Saves the settings dialog without touching identity fields — required behavior: the git name still wins after resync (block is tertiary), the dialog re-echoes the git identity, and no error surfaces; invariant: saving never downgrades a git-derived seat.
- Trigger: git binary missing or refusing to launch — required behavior: read_config returns None, connect succeeds identically to the legacy guest path (section 9: degraded but functional, never blocked), and no toast/panic is produced by the probe itself.
- Trigger: settings-Save while a turn is in flight is already refused by the single-writer UI guard; a concurrent external `git config user.name` edit takes effect only on the next resync/connect, matching the existing refresh cadence — no live identity hot-swap mid-turn is required or attempted.

## Constraints

- Pure Rust (edition 2024) with an offline-capable, minimal dependency graph — no socket or async crates at MVP; Packet initiates strictly zero network traffic (model and MCP traffic belong entirely to the operator-provisioned pi CLI's external configuration).
- Git is the sole durable store for shared planning state (no database); single-writer per process (concurrent submits refused); all planning-file writes atomic via temp-plus-rename; checkpoint subjects short and imperative; commit author Packet Planner.
- Clean harness seam: Packet orchestrates, the externally provisioned pi CLI performs all LLM interaction; discovery is PACKET_PI_BIN → PATH → common locations with a display-only, unpinned version policy (D-13).
- No quantitative performance or usability thresholds in the acceptance bar (ruled out by D-19); the §12 binding exit bar is alterable only by fresh operator ruling, never by quiet implication (D-21 anti-drift discipline).

## Out of scope

- The D-18 WebSocket real-time collaboration channel at MVP — deferred post-MVP as the first first-class post-MVP feature by D-22, carrying its open design questions CLR-015 (topology/trust), CLR-016 (writer model), CLR-017 (sharable surface vs D-07).
- Direct provider/API integration, embedded inference, and additional harness backends (seam retained via AiHarness; post-MVP intent per D-12); in-app harness installation automation (manual provisioning per the F-16 guide).
- Any platform or delivery vehicle beyond building and running from source on the operator's own Linux x86_64 workstation (other OSes, other architectures, installers, tarballs, packages — D-11, CLR-001 resolved).
- Spec editing in the UI, separate project databases, dashboards and analytics, ADR or risk registries, review/approval workflows, complex permissions or ACL engines, autonomous coding, PM-tool integrations (Contract §24).
- The timeboxed real-team pilot in the MVP exit bar (barred per D-11/D-20; reserved as first post-MVP dogfood) and a standalone scripted automated E2E harness (subsumed by the §12 demonstration plus the test suite).

## Rollout and compatibility

Backward compatible at the artifact level: no new files, no config.schema change, no persistence migration — only interpretation shifts, which is exactly what D-14 decreed (config block demoted to tertiary). Behavior-visible change for any operator whose config name differed from their git name: the git name now wins; this is the ruling, not a defect, and the dialog's provenance line makes the swap legible. Rollback is a plain revert of the commit range; reverted code restores config-first seating with zero residue because nothing on disk gains a new shape. Downstream readiness: ticket 2's router delta and the §12 demonstration both consume effective_user() as-is, so shipping this ticket first lets the F-8 wake-up conditions (git-seated session PLUS routing delta) accumulate independently.

## Definition of done

- Full offline test suite green in one run, including the new IdentitySource composer matrix, hermetic read_config probe tests, and the load/resync precedence chain, with zero edits forced onto pre-existing fixtures.
- rg 'routing_user' across src/ returns no hits and cargo build --offline prints no warnings, evidencing the third-identity collapse and the NFR-8 quality bar over the changed tree.
- On the connected tree, the live settings card visibly seats 'Zachary Barno' labeled as derived from git user.name, surviving a no-edit Save round-trip — screenshot- or transcript-verifiable by the operator per the smoke command.

## Instructions for the implementing model

Read this story and its linked dependencies before editing. Confirm the named existing files and interfaces; proposed files are explicitly labeled. Implement only this task's scope, preserving the constraints above. Run the verification commands and record actual results, changed files, and any remaining limitations. Do not mark complete on a build alone when acceptance criteria require runtime interaction. If an assumption contradicts the repository or approved specification, report the discrepancy rather than silently inventing a requirement.
