# 002 — Add persona markdown card to the Settings dialog with save and restore-default actions
<!-- packet-artifact-id:v1 {"uid":"4974d41e-2ab7-4b43-a37d-ee0b7e0ed17d","displayId":"002","title":"Add persona markdown card to the Settings dialog with save and restore-default actions","parentUid":"75b345b5-4459-4270-820d-bdbe857debaa"} -->

Feature: Editable Operator Persona (markdown persona system with shipped default voice)

Status: Individually validated; see batch index for generation status.

## Problem this ticket solves and why

After story 001 landed, the persona store is headless: no UI path calls load_persona or save_persona, so the operator has no surface to view, edit, or repair the planner voice document - no first-run visibility, no restore action, no in-app display of a corruption diagnostic. Unresolved, the approved journey (Settings, Persona, edit markdown, save) stays impossible from inside the app, story 003 could inject a document the operator can never inspect or correct, and AC1/AC2 fail by construction even with a perfect store.

## Ticket goal — what changes when done

Before: the Workspace settings modal in src/ui/layout.rs holds only queue-mechanics controls and never touches the persona store. After this ticket alone: that modal carries a Planner persona section showing the current persona.md verbatim in a markdown TextEdit (opening on a pristine home seeds the shipped four-beat default), a section Save with unchanged-detection and a keep-open error path, a prominent Restore default rewriting exactly SHIPPED_DEFAULT_PERSONA, a pinned one-line subordination notice, and sticky diagnostics. Observable completion: open on a fresh PACKET_HOME and see the seeded text plus an info line; edit and Save and the disk bytes equal the buffer with every connected repo git-clean; kill and relaunch and the edit persists; Restore default reproduces the shipped bytes exactly.

## User story

As the seated operator Zachary Barno (sole seat per D-23), I want a Persona card in the Settings dialog that shows, edits, saves, and restores the planner markdown persona with visible diagnostics, so that I tune the voice in-app, watch the shipped four-beat default appear on first run, and bring it back anytime - with no rebuild, no repository diff, and no silent blank when the file breaks.

## Purpose

The operator has no in-app surface to own the voice; this ticket gives them a standard-markdown Persona card in the existing Settings dialog mirroring the MCP editor's discipline (raw free-text edit, explicit save, keep-open diagnostics — paint_mcp_card in src/app/dialogs.rs). It must view and persist the current document, prominently offer Restore default that reinstates the shipped four-beat text, trigger the first-run seed, and carry a one-line notice that the persona tunes voice and principles while the application's protocol remains in force, completing the edit-save-relaunch journey of REQ-P1-1 and AC1/AC2.

## Specification references

Source: [Approved specification](specification.md)

## Implementation context

Grounded at base 6214a58. Verified here: the Workspace settings dialog is driven by the settings_open toggle painted at src/ui/layout.rs (header Settings entry, modal titled Workspace settings at width 560.0, body wrapped in a scrolling vertical area by the shared modal helper, height clamped to viewport minus margin); the card discipline model is paint_mcp_card at src/app/dialogs.rs:669 over struct DlgMcp at :600 (raw free-text multiline TextEdit, explicit save via footers, sticky warning, ok-plus-message feedback, char-count line, module doctrine that business effects run on SAVE only). The settings modal body scrolls, so the added card degrades to scroll height. Consumed contract from story 001 (landing file src/persistence/persona.rs, treated as complete dependency; this story edits it zero times): SHIPPED_DEFAULT_PERSONA (&str, eight LF-terminated lines spelling Inquisitive, never the Inqsitive typo), persona_path() returning persona.md at the dollar-PACKET_HOME-aware state root, struct PersonaLoad with document, seeded_now, fell_back_to_default, diagnostic Option<String>, load_persona() -> PersonaLoad, save_persona(&str) -> io::Result<()> whose blank guard returns ErrorKind::InvalidData before any disk IO and which rewrites atomically via crate::artifacts::atomic_write. Palette constants TEXT, TEXT_DIM, WARNING, SUCCESS, DANGER, PANEL_ALT, ACCENT_SOFT all exist in src/ui/theme.rs, so no new tint is owed. Stated-assumed details to confirm on first edit (cheap greps, adjust copy to match reality): the exact AppError variants available for mapping (use a string-detail variant and an io-detail variant, both already constructed elsewhere in dialogs.rs) and the exact settings_open block boundaries in layout.rs. Test idiom copied from story 001 and the dialogs tests module: static lock, pid-tagged temp home under std::env::temp_dir(), unsafe env set/unset only under the lock with prior value restored.

Feature ID: CHG-003
Repository: root

## Technical design and contracts

- New card model in src/app/dialogs.rs: pub struct DlgPersona { pub document: String, base: String, seeded_note: Option<String>, warning: Option<String>, feedback: Option<(bool, String)> }. Constructor pub fn from_load(load: &crate::persistence::persona::PersonaLoad) -> Self is a pure mapping: document and base := load.document cloned; seeded_now true sets seeded_note to a sentence naming persona_path().display(); fell_back_to_default true copies load.diagnostic into warning; feedback starts None. from_load performs zero disk IO.
- Effect methods own every state change; the painter stays inert. pub fn save(&mut self) -> Result<PersonaSaveOutcome, AppError> with pub enum PersonaSaveOutcome { Unchanged, Written }: if document == base return Unchanged with zero IO; else save_persona(&document) mapping ErrorKind::InvalidData to a friendly blank-guard error message and every other kind to an io-detail error carrying the OS message; on Written set base := document clone and clear seeded_note and warning (bytes now known-good). pub fn restore_default(&mut self) -> Result<(), AppError> calls private fn stage_default(&mut self) - pure: document := SHIPPED_DEFAULT_PERSONA.to_string(), base := same, notes and feedback cleared - then persists through save_persona; on Ok set feedback Some(true, Restored the shipped default persona.); on Err keep the staged buffer and set feedback Some(false, err text) so the operator sees the intent survived.
- Painter signature pub fn paint_persona_card(ui: &mut egui::Ui, card: &mut DlgPersona) -> (bool, bool) returning (save_pressed, restore_pressed), mirroring paint_mcp_card sizing: heading Planner persona; one line pub const PERSONA_SUBORDINATION_NOTICE: &str = Tunes the planner voice and principles only - the application envelope, routing, and safety rails stay in force. at weak 11pt; seeded_note line in TEXT_DIM and/or warning line in theme::WARNING when present; monospace multiline TextEdit over card.document at nine desired rows; a chars meter (document.chars().count()) at 10.5pt TEXT_DIM; footer row with left button Restore default (strong RichText, PANEL_ALT fill) and right button Save (ACCENT_SOFT fill); feedback line colored SUCCESS or DANGER by ok flag. No Close control - the modal X owns dismissal; the shared footers() helper used by the four existing dialogs stays byte-identical.
- Hosting and lifecycle in src/ui/layout.rs settings block: widen modal width 560.0 to 640.0; after the auto-mode explanatory labels insert ui.separator() then a new private helper paint_persona_section(ui: &mut egui::Ui, s: &mut dyn Surface). Helper idiom: detect first-ever entry via Id packet_persona_was_closed held in ctx temp data (closed true means pristine); when pristine, lazily construct DlgPersona::from_load(&persona::load_persona()), which is the first-run seed trigger; thereafter reuse the temp slot so the draft survives redraws within the open period. Signals: save pressed matches card.save() - Written fires toasts().success with Persona saved - effective from the next reply.; Unchanged fires toasts().info with Persona already in sync.; Err leaves the card open with the red feedback already set; restore pressed matches card.restore_default() with success toast Persona default restored. On the settings_open true-to-false edge (tracked with a prior-open boolean in temp data) the handler calls ui.ctx().data_mut with clear(), which drops the volatile draft, so the next open re-binds live file bytes per the bind-on-open doctrine.
- Failure and observability contract: no panic path - every fallible leg surfaces as the red feedback line with the modal open; the store diagnostic renders verbatim as the amber warning on open; reads never heal the file (story 001 invariant: only an explicit Save or Restore writes); the only debris is at most one stale atomic-write temp in the state root after a failed rename, displaced by the next successful write; the single-seat assumption (D-23) licenses last-writer-wins between a card save and story 003 per-turn reads, so no locking is added here.

## Approved scope mapping

- Scope 1: Markdown persona editor card in the existing Settings dialog (first-run seed, save, restore default).
- Success criterion 1: A standard-markdown persona card in the Settings dialog: view, edit, and save the persona document, with restore-default.
- Success criterion 2: First run seeds the shipped four-beat default: concise; protective of the user, then the system, then the project; inquisitive; creative.

## Dependencies

- [Task 001](001-add-operator-home-persona-store-with-shipped-four-beat-default-first-run-seed-and-corrupt-file-fallb.md) must be complete.

## Affected files and components

- src/app/dialogs.rs (EXISTING): gains the persona card - PERSONA_SUBORDINATION_NOTICE, PersonaSaveOutcome, DlgPersona with from_load/save/restore_default/private stage_default, paint_persona_card, and the in-file tests; the shared footers() helper and all four pre-existing dialogs stay byte-identical.
- src/ui/layout.rs (EXISTING): edits confined to the settings_open draw block - modal width 560.0 becomes 640.0, separator plus paint_persona_section(ui, s) after the auto-mode labels, the section helper defined near the existing layout helpers, and the closed-edge data-clear at the settings-toggle site.
- src/persistence/persona.rs (EXISTING via story 001; CONSUMED UNMODIFIED): imported for load_persona, save_persona, SHIPPED_DEFAULT_PERSONA, persona_path; zero edits - its tests and normative bytes must stay green and unchanged.
- src/ui/theme.rs (EXISTING; UNTOUCHED): the card composes only the already-shipping constants TEXT, TEXT_DIM, WARNING, SUCCESS, DANGER, PANEL_ALT, ACCENT_SOFT.
- src/app/root.rs and src/ui.rs (EXISTING; UNTOUCHED): no Dialog enum, HeaderAction, or Surface trait change is needed because the card is hosted inside the settings modal layout already drives and toasts() is already reached there.

## Implementation steps

1. In src/app/dialogs.rs add the persona module import beside existing use lines, then declare PERSONA_SUBORDINATION_NOTICE, PersonaSaveOutcome, and DlgPersona; implement from_load as the pure branch over the three load flags and immediately pin it with the pure-fixture tests so the mapping is locked before any painting exists.
2. Implement save() and restore_default() with private stage_default exactly per the effect-methods design - no-churn short-circuit before any IO, the two-arm error mapping preserving the OS message copy, state mutation only on Ok - and verify with the no-op and blank-gate tests, which need no environment.
3. Write paint_persona_card strictly in house painter style, borrowing sizes, fills, and layout from paint_mcp_card; return the (save_pressed, restore_pressed) pair and perform no file IO itself; keep footers() byte-identical so the existing dialogs are provably undisturbed.
4. Edit src/ui/layout.rs: raise the settings modal width to 640.0, insert the separator and paint_persona_section call after the auto-mode labels, and implement the helper with the closed-flag temp lifecycle, the seed-on-first-entry load, the signal-to-toast outcome table, and the data.clear() call on the true-to-false settings_open edge.
5. Add the single consolidated disk-effect test guarded by a new static mutex in the dialogs tests module: capture the prior PACKET_HOME value, then under the lock run phases A through E sequentially in ONE test fn over one pid-tagged parent temp home (each phase repoints the env to a fresh child dir under the parent), always restoring the prior env value and deleting the parent at end-of-function.
6. Proof of bounded blast radius before finishing: run the filtered dialogs tests, the full cargo test, the persona-filtered clippy, and confirm git status plus git diff --stat show exactly src/app/dialogs.rs and src/ui/layout.rs modified (on top of story 001's two files) with Cargo.toml, Cargo.lock, and every repository working tree byte-identical.

## Acceptance criteria

- Given a pristine PACKET_HOME with no persona file, when the operator opens Workspace settings, then the Planner persona editor shows the shipped four-beat default, an info line names the seeded persona.md path, and the state-root persona.md bytes equal SHIPPED_DEFAULT_PERSONA exactly.
- Given the card showing stored text T, when the operator replaces the buffer with a different multi-line text and presses the section Save, then the file bytes equal the new buffer exactly, a green feedback line and a success toast appear, git status porcelain in every connected repository stays empty, and killing then relaunching the app re-binds the editor to the saved text.
- Given a card opened over a healthy file, when the operator edits the buffer and dismisses the modal without pressing Save, then the file bytes are unchanged, the draft is dropped, and a concurrently made external edit to the file is what the next open displays - live re-bind, no ghost draft.
- Given persona.md holding invalid UTF-8 bytes, when the operator opens settings, then the editor shows the shipped default with the store diagnostic rendered amber and the corrupt bytes untouched by the mere open; when the operator then presses Restore default, then the file bytes equal SHIPPED_DEFAULT_PERSONA, the amber line clears, and the green Restored confirmation appears.
- Given a writable home, when the operator selects-all deletes in the editor and presses Save, then a red feedback quoting the blank guard shows, the stored bytes are unchanged, and the app stays responsive with no panic; repeating the gesture against a read-only home (after deleting the file) instead shows the mapped io error as a red line.
- Given an unedited card (zero modifications since open), when Save is pressed, then an unchanged outcome fires - no file write (mtime unchanged), no stale temp in the state root, and the info toast Persona already in sync.

## Test plan

1. Pure mapping pins (no env): hand-build three PersonaLoad fixtures - plain (flags false, diagnostic None), seeded (seeded_now true with a diagnostic), fallen-back (fell_back_to_default true with a UTF-8 diagnostic) - and assert from_load copies document bytes exactly, mirrors base, populates seeded_note iff seeded_now and contains a persona_path display substring, populates warning iff fell back, and starts feedback None.
2. Pure restore staging: build a card from a custom altered document, call stage_default (same-module privacy), assert document bytes equal SHIPPED_DEFAULT_PERSONA, notes and feedback are None, and the constant contains Inquisitive while it lacks the Inqsitive misspelling.
3. No-op and blank gate (no env): an unedited card returns save() == Unchanged; then set document to the empty string, to spaces plus tab plus newline, and to spaces only, asserting save() is Err with the blank-guard text in each case while document itself is unchanged.
4. Consolidated disk effect under the static lock in one test fn, prior env captured and always restored, phases over sibling child temp homes: A fresh home - load_persona seeds on disk with constant bytes and seeded_note Some; B unmodified card - save() == Unchanged and the home root lists exactly persona.md; C custom document with bullets, bold markers, and an em dash - save() == Written, file bytes byte-equal, warning cleared; D whitespace buffers - three blank attempts Err and bytes still equal Phase C; E corrupt home - write invalid UTF-8, from_load warns with the diagnostic mirrored, restore_default Ok, file equals the constant, warning None, feedback positive; also assert a pre-phase file mtime is unchanged after Phase B (no write churn).
5. Subordination copy pin: assert PERSONA_SUBORDINATION_NOTICE contains the words voice, principles, and rails, defending the mandated one-line notice against later rephrases.
6. Incumbent regression: existing app::dialogs tests (echo and golden-pin suites) run unmodified and green under the lib test run, plus the persona-filtered clippy check returns empty.

## Verification commands and expected evidence

1. Working directory /mnt/DevProj/Packet: cargo test --lib app::dialogs -- expected: every persona section test passes with 0 failed on the host Rust 1.98 toolchain matching the D-34 pin.
2. Working directory /mnt/DevProj/Packet: cargo test -- expected: full suite (lib units plus multi_repository_feature and task_workflow integration targets) green with zero new failures against base.
3. Working directory /mnt/DevProj/Packet: cargo clippy --all-targets 2>&1 | rg -i persona -- expected: empty output, meaning no warning names a symbol this story introduced (the formal D-34 sorted-listing baseline diff executes in story 005).
4. Working directory /mnt/DevProj/Packet: git status --porcelain && git diff --stat -- expected: exactly src/app/dialogs.rs and src/ui/layout.rs modified on top of story 001's src/persistence persona files; Cargo.toml and Cargo.lock unchanged.
5. Scripted manual walkthrough (not part of the automated suite): export PACKET_HOME to a fresh mktemp dir, build and launch the app against the seeded fixture repository, open settings and confirm the seeded card; edit and Save; kill the process; relaunch; confirm the editor shows the edited text and git status porcelain in the fixture repo is empty.

## Edge cases and failure handling

- Select-all-delete plus Save (blank buffer): the store InvalidData preflight trips before any disk IO; the card maps it to the friendly red line; buffer and file both stand as-is and a subsequent Save with real text succeeds normally.
- Concurrent reader: a second instance or story 003 per-turn assembly reads persona.md while the card writes - atomic temp-plus-rename guarantees readers observe only whole-old or whole-new bytes; last-writer-wins is licensed by the D-23 single seat and this ticket adds no locking.
- Narrow window: the modal clamps to viewport minus margin and its body scrolls, and the editor follows available width, so the added section degrades to extra scroll with no horizontal clipping or forced window growth.
- Restore default while a fallback is active: the restore write is the only sanctioned healer - it overwrites corrupt bytes, and warning plus seeded note clear only on a successful write; on a read-only home the buffer keeps the staged constant and a red line explains the failure, preserving story 001 never-heal-on-read.
- Workspace switched away (disconnect) while the modal is conceptually open: the settings draw halts, the temp-data draft is session-volatile and dies with the next open cycle per the closed-edge data.clear(), and the next open re-binds live file bytes - deterministic re-seed of the view, no stale draft resurrection.
- Large paste (multi-KiB markdown): the field renders at nine desired rows and scrolls, Save persists verbatim with no size cap or truncation (consistent with story 001), and the char meter costs one cheap chars().count() per frame.

## Constraints

- Scoping is operator level — operator ruling; storage must live outside the repository.
- The shipped default is the operator's four beats (typo 'Inqsitive' normalized to 'Inquisitive').
- Boundary ruled Option 1 (CLR-022, DE-3): the persona is a subordinate voice-and-principles overlay; envelope, routing/veto, board protocol, and safety rails stay inviolate.
- No new crates; offline-minimal posture kept; envelope validation layer untouched.

## Out of scope

- Per-project or repo-committed/shared personas (contradicted by the operator-level ruling).
- Persona libraries/pickers, per-conversation overrides, version history.
- Multi-operator persona profiles (one seated operator per D-23; storage may reserve a seam).
- Distributing personas over the deferred D-18 channel; auto-suggested personas.

## Rollout and compatibility

Purely additive with no migration: the card only fronts a store that owns its own seeding semantics, so existing installs see persona.md appear on first dialog open (or earlier if story 003's turn loads fire first) with identical bytes either way, and no previously persisted state is touched. Persona save deliberately creates no git checkpoint and writes no repository file (different from the MCP and stakeholder savers), per the operator-level ruling; every connected repository working tree and .git directory stays clean. Rollback is reverting the two-file diff; the temp-data card state is session-volatile and evanesces on its own, and an orphaned seeded persona.md is harmless since loading it merely serves the shipped voice. Compatibility: the four pre-existing dialogs are byte-identical, no enums or traits changed, no manifest delta, palette constants all pre-existing.

## Definition of done

- cargo test --lib app::dialogs exits zero on the pinned Rust 1.98 toolchain, covering every test_plan entry including the consolidated disk-effect phases A through E and all negative paths.
- Full cargo test green at the working tree, the persona-filtered clippy output empty, and git diff --stat confined to src/app/dialogs.rs plus src/ui/layout.rs with Cargo.toml and Cargo.lock byte-identical.
- Live walkthrough evidences the frozen acceptance criteria: first-open seed visible, edited text persisting across relaunch, Restore default reproducing the shipped bytes exactly, and every connected repository git status clean.
- No new clippy warning names a persona-introduced symbol, keeping this story's delta clean for story 005's pre-work baseline-diff gate.

## Instructions for the implementing model

Read this story and its linked dependencies before editing. Confirm the named existing files and interfaces; proposed files are explicitly labeled. Implement only this task's scope, preserving the constraints above. Run the verification commands and record actual results, changed files, and any remaining limitations. Do not mark complete on a build alone when acceptance criteria require runtime interaction. If an assumption contradicts the repository or approved specification, report the discrepancy rather than silently inventing a requirement.
