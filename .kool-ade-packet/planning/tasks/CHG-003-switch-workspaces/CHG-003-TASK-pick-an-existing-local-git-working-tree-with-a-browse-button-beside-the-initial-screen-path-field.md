# CHG-003-TASK-pick-an-existing-local-git-working-tree-with-a-browse-button-beside-the-initial-screen-path-field — Pick an existing local git working tree with a browse button beside the initial-screen path field
<!-- packet-artifact-id:v1 {"uid":"942a8ce6-1978-45ec-a9c7-05519a22b645","displayId":"CHG-003-TASK-pick-an-existing-local-git-working-tree-with-a-browse-button-beside-the-initial-screen-path-field","title":"Pick an existing local git working tree with a browse button beside the initial-screen path field","parentUid":"474c99dc-369a-4425-a490-f8443b37e774"} -->

Feature: Switch Workspaces

Status: Individually validated; see batch index for generation status.

## Problem this ticket solves and why

On Packet's initial screen the only way to name a workspace is the hand-typed monospace path field in welcome::paint (src/app/welcome.rs, singleline TextEdit with hint '/path/to/my/project'); operators must discover absolute paths in external tools and retype them, and every miss burns a round-trip through attempt_connect's InvalidRepo banners (not a directory / 'no .git directory found'). Leaving path entry purely textual keeps first connect dependent on muscle memory and forces the browse-by-hand workaround this feature exists to remove.

## Ticket goal — what changes when done

Beside the initial-screen path field, a 'Browse…' button opens a modal directory browser (reusing overlays::show_modal and the Dialog/placement conventions of Import/Settings/Mcp). From it the operator descends folders, sees which are git working trees (live gitops::is_work_tree marks), and 'Choose folder' writes the selected absolute path into the existing PacketApp.conn_path string and closes the modal — submitting nothing. Complete when: the field shows the browsed canonical path, the screen is still the initial screen with no toast/connect side effects, and the existing Open button/Enter via submit_connect remains the single connect authority.

## User story

As the seated operator (Zachary Barno, D-23) sitting on Packet's initial screen, I want a Browse button beside the path field that opens a folder browser and marks which folders are git working trees, so I can pick an existing local repository and drop its absolute path into the field without hunting for the path in a file manager and risking typos.

## Purpose

The initial screen (src/app/welcome.rs) offers only a hand-typed path field, forcing operators to hunt for absolute paths in external tools and mistype them, with no way to discover candidates; this task adds a browse button beside the path field opening a directory-browser dialog that lets the operator navigate folders, indicates which visited directories are git working trees using the system git CLI probe already used by gitops::is_work_tree, and places the chosen path into the existing conn_path string without auto-submitting, leaving the Open button and Enter submission as the single connect authority.

## Specification references

Source: [Approved specification](specification.md)

## Implementation context

Verified at root commit ead188a: PacketApp holds conn_path: String (src/app/root.rs:29), defaulted to std::env::current_dir() (:111); the Welcome paint arm (:1904-1919) wraps welcome::paint(card_ui, path, error) in a 460-wide theme card and calls submit_connect() on the returned bool; submit_connect (:1019-1035) skips empty paths, runs welcome::attempt_connect, toasts + switches to Screen::Connected on Ok, stores '{headline}\n{detail}' in conn_error on Err. Modals: enum Dialog {Import,Settings,Mcp} (root.rs:81), render_dialog (:1936+) drives overlays::show_modal(ui, true, title, width, body) with RefCell save/close slots and re-stores the dialog unless closed or positively completed; overlays.rs builds an egui::Modal (foreground order, X close, Escape-close, scroll body) and its in-file tests show the ctx.run_ui frame-driving pattern. House dialog style (src/app/dialogs.rs): DlgNew-type structs with constructors, paint_*_card returning pressed-flag tuples, a footers() button recipe, and a private expand_tilde() (line 1477). Git probing: gitops::is_work_tree(cwd: &Path) -> bool (src/core/gitops.rs:45-55) shells 'git -C <cwd> rev-parse --is-inside-work-tree' via a vector-argument Command (NFR-5 compliant, no shell). attempt_connect trims, expands '~/', canonicalizes, then checks is_dir + is_work_tree — the browser must NOT pre-judge non-git folders. Theme already defines all needed colors (theme::SUCCESS rgb(107,212,144), TEXT, TEXT_DIM, ACCENT_SOFT, BG, PANEL, BORDER); no new constants are needed. egui 0.36.1 / eframe 0.36.1, rustc 1.98.1 in use, offline cargo works (lib suite ~394 tests, fast). Out of scope for this ticket: the GitHub URL clone entry (story 3), remembered/recents lists (brief decline), any change to attempt_connect/submit_connect semantics, Connected-screen UI, and new Cargo dependencies. NOTE: the prompt's 'APPROVED FEATURE SPECIFICATION' text describes an unrelated chat-formatting feature; the approved brief, frozen outline, and current task entry above (Switch Workspaces, scope item 2) are the governing authority for this story.

Feature ID: CHG-003
Repository: root

## Technical design and contracts

- Change welcome::paint's signature (currently pub fn paint(card_ui: &mut egui::Ui, path: &mut String, error: Option<&str>) -> bool, sole caller root.rs:1911) to pub fn paint(card_ui: &mut egui::Ui, path: &mut String, error: Option<&str>, browse_requested: &mut bool) -> bool; replace the full-width TextEdit block with egui::Ui::horizontal containing the same singleline TextEdit (hint, monospace 12.5, desired_width INFINITY, height 42) plus a fixed 96x42 egui::Button labeled 'Browse…' (ellipsis already renders in this UI, cf. dialogs.rs:262); set *browse_requested on click; the Enter-detection and submit-return lines stay byte-identical, and while the modal is open egui::Modal's input steal makes the card's Enter hook inactive automatically.
- Add Dialog::Browse(DlgBrowse) to the enum at src/app/root.rs:81 (compiler-enforced exhaustiveness catches render_dialog); in the Screen::Welcome arm capture the flag via a RefCell slot and push self.dialog = Some(Dialog::Browse(DlgBrowse::seeded(self.conn_path.clone()))) after paint; add a render_dialog arm mirroring the Import arm: RefCell<bool> choose_slot/cancel_slot feeding overlays::show_modal(ui, true, 'Choose a workspace folder', 560.0, |ui| { dialogs::paint_browse_card(ui, &mut d) }), then on *choose_slot set self.conn_path = d.selection().to_string_lossy().into_owned() guarded by matching Screen::Welcome (defensive; only Welcome pushes), and re-store Some(Dialog::Browse(d)) iff !closed && !*cancel_slot, exactly the existing put-back convention.
- Define in src/app/dialogs.rs: pub struct DlgBrowse { current: PathBuf, selected: PathBuf, rows: Vec<DirRow>, read_error: Option<String>, git_cache: HashMap<PathBuf, bool> } with struct DirRow { path: PathBuf, up: bool, git: bool }; ctor DlgBrowse::seeded(seed: String) delegates to a pure, unit-testable resolver pub(crate) fn resolve_seed(seed: &str, home: Option<&Path>, root: &Path) -> (PathBuf, PathBuf) encoding the precedence: trimmed seed expanded via the existing private expand_tilde (dialogs.rs:1477, reused in-file — do not duplicate welcome::expand_home) -> (a) exists-as-dir: canonicalize, current==selected==it; (b) parent() exists-as-dir: current==selected==canonical(parent); (c) canonical(home) if Some and a dir, else canonical(root=='/') — selected is therefore always an existing directory, so Choose is valid from frame one.
- refresh_rows(&mut self) (called by seeded() and every navigation): std::fs::read_dir(current); keep entries where path().is_dir() (follows symlinks); unreadable read_dir sets read_error (displayed dim) and empties rows instead of panicking; include ALL subdirectories (dot-dirs included — deliberate presentation choice, no exclusion rule), sorted case-insensitively by file-stem; git flag = gitops::is_work_tree(&p) memoized in git_cache keyed by absolute path, computed once per navigation (O(rows) git probes max), never during paint; a synthetic up row { path: canonical(parent), up: true, git: false } is prepended iff canonical(parent) != current (omitted at filesystem root, which also terminates any symlinked-parent ascent).
- Painter pub fn paint_browse_card(ui: &mut egui::Ui, dlg: &mut DlgBrowse) -> (bool, bool) // (choose, cancel) in dialogs.rs, matching the (save, close) tuple convention of paint_import_card: legend label 'Directories only. Green names sit inside a git working tree.'; ScrollArea::vertical row list where each row is ui.selectable_label(dlg.selected == row.path, RichText name colored theme::SUCCESS if row.git else theme::TEXT) — color-based git marking is deliberate: overlays.rs draws its close-X as hand strokes because the font lacks some glyphs, so no exotic Unicode in row labels; single click sets selected only; double_clicked() or modal-scoped Enter navigates (refresh_rows, selected := current); optional up-row click navigates up; bottom bar shows monospace-12.5 selected path (truncated, full path via on_hover_text) then right-to-left buttons: flat 'Cancel' -> (false, true), primary 'Choose folder' (fill theme::ACCENT_SOFT, label theme::BG, corner_radius 6, the footers() recipe) enabled iff selected.exists() && selected.is_dir() -> (true, false); Escape/X route through show_modal's closed flag to the cancel branch in render_dialog.
- Insertion contract and invariants: choosing writes ONLY conn_path (absolute canonical PathBuf -> lossy String); it never calls welcome::attempt_connect or submit_connect, never touches conn_error (the red banner keeps describing the last ATTEMPTED connect until the next Open), never switches screens; welcome's submit flow (empty-path early return, success toast 'Connected to {title}', error mapping at root.rs:1031) is the single connect authority; no Project/PlannerState/persistence fields change; no Cargo.toml changes (std::fs, std collections, egui primitives, existing gitops only).

## Approved scope mapping

- Scope 2: Directory browser on the initial screen (browse button beside the path field): navigate folders, indicate which are git working trees, place the selection into the existing path field without auto-submitting
- Success criterion 2: From the initial screen the operator selects an existing local git working tree using the browse button (directory browser) and opens it.

## Dependencies

None. This task can start independently.

## Affected files and components

- src/app/welcome.rs (existing): add the 'Browse…' button in a horizontal row with the existing singleline path field, add the browse_requested: &mut bool out-parameter to paint, keep hint text/font/sizes and Enter-submit logic unchanged, update in-file tests for the new arity.
- src/app/dialogs.rs (existing): add DlgBrowse, DirRow, seeded(), resolve_seed(), refresh_rows(), selection(), paint_browse_card() with legend/list/path-bar/buttons, plus in-file unit tests for seeding, row ordering, git marking and unreadable-folder degradation.
- src/app/root.rs (existing): add Dialog::Browse variant, push it from the Screen::Welcome arm (root.rs:1904-1919) when the flag is set, add the render_dialog arm performing conn_path injection and the standard put-back-unless-done bookkeeping.
- src/ui/theme.rs (existing, NO CHANGE): SUCCESS/TEXT/TEXT_DIM/ACCENT_SOFT/BG/PANEL/BORDER already express the marking, list and button styles; listed to document that no new palette constants are introduced.

## Implementation steps

1. Ground the fixtures first: mkdir -p /tmp/swtest/{repoA,nested,plainB} && git -C /tmp/swtest/repoA init -q, keep /tmp/swtest/plainB non-git; use std::env::temp_dir() joined with a pid+nanos stamp for tests (no tempfile crate exists in Cargo.toml) and best-effort cleanup, so git-probe assertions run on the real system git exactly as production does.
2. Implement resolve_seed in src/app/dialogs.rs as a pure function (seed, home, root inputs) covering: valid dir, non-dir file -> parent, bogus seed -> home, no-home -> '/', all canonicalized; unit-test each tier with temp fixtures so no test mutates process environment (avoids set_var races).
3. Implement DlgBrowse::seeded (thin IO wrapper over resolve_seed using std::env::var_os('HOME'), root '/') and refresh_rows(): read_dir, directory-only filter, case-insensitive sort, up-row rule, git_cache-memoized gitops::is_work_tree flags, read_error capture; unit-test row order [.hidden, aa, BB] style, up omission at '/', git flags true for repoA and its descendants and false for plainB.
4. Implement paint_browse_card in dialogs.rs: legend, scrolling selectable rows (single-click select, double-click/Enter descend, up-row ascends), Success-color git marking, read_error dim line, path bar with hover-full-text, Cancel + 'Choose folder' footer returning (choose, cancel); verify it hosts cleanly under overlays::show_modal by the same ctx.run_ui frame pattern the overlays tests use.
5. Modify src/app/welcome.rs paint: swap the add_sized TextEdit block for ui.horizontal { flex TextEdit (unchanged styling) + 96x42 'Browse…' button }, add the browse_requested parameter, leave the Enter/click submit expression untouched; update the only caller at root.rs:1911.
6. Wire src/app/root.rs: Dialog::Browse variant; in the Welcome arm read the slot flag after paint and push Dialog::Browse(DlgBrowse::seeded(self.conn_path.clone())); add the render_dialog arm with choose/cancel slots, conn_path injection on choose, and the !closed && !cancel re-store — matching the Import arm line-for-line in spirit.
7. Run cargo test --offline --lib (fast iteration on app::welcome and app::dialogs filters), then full cargo test --offline; capture the clippy warning listing before the first edit and after, confirming zero added lines and no warning naming browse symbols (see verification_commands); finish with the cargo run --offline manual smoke described in verification_commands.

## Acceptance criteria

- Given the initial screen with any text in the path field, When I click 'Browse…', Then the modal titled 'Choose a workspace folder' opens over an input-blocking backdrop with a legend line and a list of subdirectories of the seeded directory; cancelling via X, Escape or 'Cancel' leaves conn_path byte-identical, shows no toast, and attempts no connect.
- Given a machine where /tmp/swtest/repoA is an initialized git tree, When I open Browse, descend into repoA, single-click a row, and press 'Choose folder', Then the modal closes, conn_path equals the canonical absolute path of that directory, the screen is still the initial screen with no toast, and a subsequent Open/Enter runs the existing submit_connect and connects normally.
- Given a seeded directory containing 'aa', 'BB', '.hidden' subdirectories, one loose file, and one uninitialized 'plainB' folder, When the browser lists it, Then rows appear in case-insensitive name order excluding the loose file (plus the leading up row where applicable), '.hidden' is included, and exactly the rows inside a git working tree render in theme::SUCCESS while others render in theme::TEXT.
- Given the browser pointed at a directory that is NOT a git working tree, When I choose it, Then the path is inserted identically to a git tree (validation stays with attempt_connect): pressing Open afterwards reproduces the existing 'no .git directory found' InvalidRepo banner in the card's red box — the browser neither enables nor disables that path.
- Given the current folder ceases to exist while the modal is open, When I navigate or repaint, Then the list area shows a dim 'cannot read' notice with the OS error text, the up row (if any) still works, and 'Choose folder' is disabled while the remembered selection no longer exists — no panic, no crash.
- Given a pristine start where conn_path defaulted to the app's current working directory (root.rs:111), When I open Browse, Then it seeds from that directory; if the seed path is gone it falls back to $HOME, and if HOME is unset to '/' — every case still presents at least the up-less root listing and an immediately selectable current directory.

## Test plan

1. Unit (pure): resolve_seed matrix over temp fixtures — existing dir -> (canon, canon); existing file -> (canon(parent), canon(parent)); nonsense string -> (canon($HOME), same); home=None -> ('/','/') — asserting both returned paths and canonical form, no environment mutation.
2. Unit: build temp root with aa/, BB/, .hidden/, notes.txt, repoA/ (git -C repoA init -q) and a child repoA/sub/; assert refresh_rows order (leading up row excluded at seeded level: ['.hidden','aa','BB','notes?'] exact stem list, file dropped), up-row presence after descending, and git flags: repoA true, repoA/sub true, siblings false, using the real gitops::is_work_tree probe.
3. Unit: degradation — seeded at a non-existent path yields rows empty, read_error Some(non-empty), selected still the last-resolved dir (HOME/'/'), and navigating up clears read_error once a readable dir is reached.
4. Frame-driven (mirrors overlays.rs tests using egui::Context::run_ui + screen_rect): host paint_browse_card inside overlays::show_modal for two frames; frame 1 with no events must render without panic and report closed=false; frame 2 delivering Event::Key(Escape, pressed) must yield closed=true; a third scenario clicks through painter state to confirm single-click select-then-(true,false) on 'Choose folder' and (false,true) on 'Cancel'.
5. Regression proof: cargo test --offline --lib passes including incumbent welcome/dialogs tests; full cargo test --offline green; clippy diff per the commands below adds zero lines — proving the paint-signature change and new variant broke no existing caller.
6. Manual smoke (scripted): fresh app -> initial screen -> click 'Browse…' -> descend into /tmp/swtest -> choose repoA -> field shows canonical path, no toast -> press Open -> 'Connected to …' toast; then restart, browse to /tmp/swtest/plainB, choose, Open -> exact legacy 'no .git directory found' banner; Cancel path leaves the field untouched.

## Verification commands and expected evidence

1. Working directory /mnt/DevProj/Packet: cargo test --offline --lib -- app::welcome app::dialogs — expected: all tests pass, including the new browse seeding/order/mark/degradation/frame tests and the two incumbent connect-error tests.
2. Working directory /mnt/DevProj/Packet: cargo test --offline — expected: entire offline suite green (integration tests task_workflow.rs and multi_repository_feature.rs included).
3. Working directory /mnt/DevProj/Packet, run BEFORE first edit: cargo clippy --offline --all-targets --quiet 2>&1 | grep -E '^warning' | sort | uniq -c | sort -rn > /tmp/clippy.baseline — the measured pre-change count at ead188a is 108 warning lines (D-34's recorded baseline is 110 at 3ba5aa2); after the change rerun into /tmp/clippy.after and diff — expected: zero added lines and no line naming DlgBrowse, paint_browse_card or browse-related symbols.
4. Working directory /mnt/DevProj/Packet: cargo run --offline — expected: initial screen shows 'Browse…' beside the path field at 460px card width with no layout breakage; the scripted smoke in test_plan step 6 observes choose-without-connect, successful Open, and the legacy non-git banner.

## Edge cases and failure handling

- Unborn repository (git init with zero commits): git rev-parse --is-inside-work-tree still exits 0, so the row IS marked green — correct, because attempt_connect's gate is is_work_tree, not commit history, and choosing it connects today.
- Nested working trees: rev-parse walks upward, so children of repoA also probe true and render green; that matches is_work_tree's documented 'inside a work tree' semantics and must not be 'fixed' to root-only.
- Very wide directory (thousands of subdirs): refresh_rows performs up to N git probes ONCE per navigation, memoized per absolute path for the dialog's life; probe latency is bounded by N and is a documented v1 tradeoff — no per-frame probing ever, no async spawning in this ticket.
- Paths with spaces, non-UTF8 components or '~': selection travels as a canonical PathBuf and is serialized with to_string_lossy into conn_path; attempt_connect's existing trim/expand_home/canonicalize pipeline then treats it exactly like a typed path, and the path bar truncates with on_hover_text for the full value.
- Rapid successive navigation (double-click chains or immediate up-row mashing): all state transitions happen synchronously inside one paint pass on the UI thread with no workers or locks, so each committed event re-reads rows before the next — the last event wins deterministically, mirroring how DlgMcp survives consecutive saves.
- Repeated open-cancel-open cycles: each push builds a fresh DlgBrowse seeded from the CURRENT conn_path, so the browser never caches stale selections across sessions of itself, and the git_cache dies with the dialog.

## Constraints

- Clone and repository probing use the system git CLI with vector arguments (no shell interpolation), consistent with D-03 and NFR-5

## Out of scope

- Remembered workspaces / a recents list of previously opened workspaces (declined by operator in interview)

## Rollout and compatibility

Single-binary incremental rollout; no persistence, schema, artifact, checkpoint-authorship or Cargo.toml changes, hence no migration or data-compatibility step. Running Connected instances are untouched — the control only exists on the initial screen, and the modified welcome::paint is used nowhere else. No feature flag (the repository has no flagging mechanism and the brief imposes none). Rollback is a plain revert of the three touched source files; conn_path behavior on older binaries is byte-compatible since the stored value is still a plain path string.

## Definition of done

- Full offline test suite green via the recorded commands, with the new browse unit and frame tests present and passing.
- Zero added clippy warning lines versus the /tmp/clippy.baseline capture and no warning citing any new symbol; git diff --stat shows only src/app/welcome.rs, src/app/dialogs.rs and src/app/root.rs.
- Scripted smoke complete: browse->choose inserts canonical path with no connect side effect, Open then connects, a non-git choice yields the exact pre-existing InvalidRepo banner, and Cancel restores a byte-identical field.
- Browser rejects nothing that attempt_connect accepts (non-git and unborn repos choosable), selected is always canonical/absolute, and submit_connect remains demonstrably the only connect trigger on the initial screen.

## Instructions for the implementing model

Read this story and its linked dependencies before editing. Confirm the named existing files and interfaces; proposed files are explicitly labeled. Implement only this task's scope, preserving the constraints above. Run the verification commands and record actual results, changed files, and any remaining limitations. Do not mark complete on a build alone when acceptance criteria require runtime interaction. If an assumption contradicts the repository or approved specification, report the discrepancy rather than silently inventing a requirement.
