# CHG-003-TASK-spawn-a-detached-second-packet-instance-from-the-workspace-menu-open-workspace-action — Spawn a detached second Packet instance from the Workspace menu Open workspace action

- Status: Accepted
- Date: 2026-09-21
- Ticket: `planning/tasks/CHG-003-switch-workspaces/CHG-003-TASK-spawn-a-detached-second-packet-instance-from-the-workspace-menu-open-workspace-action.md`
- Implementation commit: `pending`

## Context

This decision implements the approved Packet task and records the architectural intent that was carried into the verified change.

# CHG-003-TASK-spawn-a-detached-second-packet-instance-from-the-workspace-menu-open-workspace-action — Spawn a detached second Packet instance from the Workspace menu Open workspace action

Feature: Switch Workspaces

Status: Individually validated; see batch index for generation status.

## Problem this ticket solves and why

A running Packet window is hard-locked to exactly one connected project, and the only escape hatch is the understated Workspace-menu 'Disconnect' item, whose handler (PacketApp::disconnect, src/app/root.rs:1168) calls request_cancel() on the active turn and every active implementation, warns 'Turn aborted; disconnected from …', then yanks the window back to the Welcome screen. An operator (the seated chair, D-23) who wants to drive a second project must therefore destroy in-flight planner work and hand-retype a path afterward; there is no way to view two projects at once at all. Leaving this unresolved means every context switch burns the running turn and any unsent drafts are stranded in a window that no longer has a session.

## Ticket goal — what changes when done

After this ticket alone, clicking 'Open workspace' in the Workspace menu of a connected window spawns a second, fire-and-forget Packet process that boots to the initial connect screen while the invoking window keeps its session, draft, in-flight turn, queue and worker polls completely untouched. Observable completion: two Packet windows are simultaneously alive — the older one still showing a progressing '● Working' status for its running turn, the younger one parked on the initial screen awaiting a project path — and closing either window leaves the other running normally.

## User story

As the seated operator driving multiple projects through Packet, I want the Workspace menu of my running window to offer 'Open workspace', which pops a fresh detached Packet instance on the initial screen without canceling anything in this window, so I can switch to another project while the first one's turn keeps running uninterrupted.

## Purpose

Today the only way to work on another project from a running window is the under-discovered Disconnect item, which aborts in-flight turns and drops the operator back to hand-typing a path; this task adds an 'Open workspace' entry to the Workspace menu (alongside Settings…/Disconnect in src/ui/layout.rs) whose handler spawns the running binary as a fire-and-forget, detached process — inheriting the launch and runtime-modified environment (PACKET_HOME, PACKET_TURN_TIMEOUT_SECS, PACKET_PI_BIN) — so the new window simply boots fresh on the initial screen, per PacketApp::default()'s Screen::Welcome, while the caller's in-flight turn, queue and worker polls continue undisturbed.

## Specification references

Source: [Approved specification](specification.md)

## Implementation context

Verified in this tree (master, base ead188a): src/main.rs runs eframe::run_native("Packet — git-native specification planner", packet::app::options(), |_| PacketApp::default()) and never inspects argv, so a bare self-spawn deterministically lands on the initial screen; PacketApp::default() (src/app/root.rs:108) initializes screen: Screen::Welcome, which paints src/app/welcome.rs (hand-typed path field + Open button into welcome::attempt_connect, which validates a git work tree, recovers transactions, loads PlannerState, bootstraps the scaffold, hydrates chat). The Workspace menu (src/ui/layout.rs:43-57) offers 'Settings…', a separator, then ('Import references','Stakeholders & ownership','MCP servers','Refresh repository','Disconnect'), each routed through the Surface trait's on_header_action (src/ui.rs:124) to the single real implementation — PacketApp's exhaustive match at src/app/root.rs:1847-1879 (the task_chat test mock at src/ui/task_chat.rs:898 wildcards the argument, so no second match to update). disconnect() is the counter-example to imitate negatively: it must NOT be invoked and its cancel/abort/clear behavior must stay reachable as today. Env knobs are process-env-read at use time: PACKET_HOME (src/persistence/home.rs:12), PACKET_TURN_TIMEOUT_SECS (src/core/turn.rs:34, at turn start, 12 h fallback per D-24), PACKET_PI_BIN (src/harness/pi_harness.rs discovery, D-13); grep-verified, every env::set_var/remove_var occurrence outside tests lives in the drop-scoped gitops::Shield guard around child git execs, so a child inheriting the stock environment (no .env_clear) faithfully reproduces the operator's launch env. Repo-grep finds no single-instance lock, pipe or mutex guarding a second process; concurrent instances are structurally safe: chat persistence is per-slug under $PACKET_HOME and written via atomic rename-swap (src/persistence/chat_store.rs:82 → crate::artifacts::atomic_write, NFR-2), and same-repo queuing contends through the existing filesystem lock (Project.queue_lock, src/app/session.rs). ToastQueue (src/app/root.rs:28 field; info/warning constructors at src/ui/toast.rs:69,75) gives the feedback channel. Toolchain: rustc/cargo 1.98.1 present locally, matching the D-34 pin; package name packet, edition 2024. Constraints: std-only (std::process::Command, std::env::current_exe) — zero new Cargo.toml dependencies; primary platform Linux x86_64 per D-11. PROPOSED (not yet in tree): module src/app/spawn.rs, HeaderAction::OpenWorkspace, and a PacketApp spawn-target test seam.

Feature ID: CHG-003
Repository: root

## Technical design and contracts

- Add variant OpenWorkspace to pub enum HeaderAction (src/ui/layout.rs:6-13). The only exhaustive consumers are the PacketApp match at src/app/root.rs:1847 and the tuple array in the menu painter; the compiler enforces completeness, and the task_chat.rs:898 test mock uses a _action wildcard so it compiles untouched.
- Insert the tuple ("Open workspace", HeaderAction::OpenWorkspace) into the layout.rs menu-button tuple array (lines 50-56), positioned immediately before ("Disconnect", HeaderAction::Disconnect) so the two contrasting workspace-lifecycle actions sit together; the label is exactly 'Open workspace'. The entry is intentionally ungated — no is_busy() guard — because opening a sibling must remain possible WHILE a turn is in flight.
- Create new leaf module src/app/spawn.rs (registered via pub mod spawn; in src/app.rs) exposing two pure functions: pub fn resolve_self_executable() -> std::result::Result<std::path::PathBuf, String> wrapping std::env::current_exe() (on Linux this is the /proc/self/exe resolved path, no symlink chase needed), with an error string naming the failure; and pub fn spawn_sibling(bin: &std::path::Path) -> std::result::Result<(), String>.
- spawn_sibling builds std::process::Command::new(bin) with .stdin(Stdio::null()), .stdout(Stdio::null()), .stderr(Stdio::null()) — null stdio is mandatory because the Child handle is dropped immediately and an un-drained pipe buffer would stall the child once filled — with the DEFAULT inherited environment (never .env_clear(); that is what carries PACKET_HOME, PACKET_TURN_TIMEOUT_SECS, PACKET_PI_BIN plus WAYLAND_DISPLAY/DISPLAY into the sibling), then .spawn() and drops the Child without ever calling wait/wait_for/pid tracking. Fire-and-forget: the parent must never hold, poll, or reap the child; on Linux the sibling outlives a closed first window (kernel reparents surviving children), and only a negligible zombie window exists if a sibling exits while the parent lives. Error strings embed the attempted binary path for the operator-facing toast. Platform boundary: Linux x86_64 is the target (D-11); Windows console-flash behavior is assumed out of scope and unverified.
- Add to PacketApp (near fn disconnect, src/app/root.rs ~1168): pub fn open_workspace(&mut self) which computes the target binary as self.spawn_target_override.clone().and_then(clone) if Some, else resolve_self_executable()? flattened into a Result — where spawn_target_override: Option<std::path::PathBuf> is a new PacketApp field (initialized None in default(); ALWAYS None in production; the only writers are #[cfg(test)] tests pointing it at a benign helper binary). On Ok it pushes self.toasts.info("Opening a new Packet window"); on Err(msg) it pushes self.toasts.warning(msg). The new match arm HeaderAction::OpenWorkspace => self.open_workspace(), in on_header_action (root.rs:1847-1879) must perform NOTHING else: no active_turn/active_implementations cancellation, no self.screen/self.dialog/self.synch/self.queue mutation — spawn and toast only, which is the contract difference versus disconnect().

## Approved scope mapping

- Scope 1: Workspace-menu 'Open workspace' action: detached self-spawn of the running binary; the spawned instance inherits the launch environment (PACKET_HOME, PACKET_TURN_TIMEOUT_SECS, PACKET_PI_BIN) and lands on the initial screen
- Success criterion 1: Pressing 'Open workspace' in the Workspace menu of a connected workspace spawns a second, detached Packet instance whose window presents the initial screen, while the first instance's in-flight work continues undisturbed.

## Dependencies

None. This task can start independently.

## Affected files and components

- src/ui/layout.rs (existing): add the OpenWorkspace variant to HeaderAction and the 'Open workspace' menu row above 'Disconnect' in the Workspace menu_button tuple array.
- src/app/spawn.rs (NEW): resolve_self_executable and spawn_sibling per the design, plus in-file #[cfg(test)] unit tests for the spawn/error/inheritance behavior using an injected binary path.
- src/app.rs (existing): register the new leaf module with a single pub mod spawn; line in the existing pub mod list.
- src/app/root.rs (existing): PacketApp field spawn_target_override (init None), open_workspace method, the new on_header_action match arm, and in-file UI tests exercising menu wiring and the error path through the existing fixture/frame/click_text/text_position helpers.
- Explicitly NOT touched: src/core/**, src/harness/**, src/app/welcome.rs (browse button is the next ticket), Cargo.toml (no dependencies may be added), src/ui/toast.rs, src/ui.rs.

## Implementation steps

1. Step 1 — define the plumbing: create src/app/spawn.rs with resolve_self_executable() (wrapping std::env::current_exe with an error string that includes the std::io::Error) and spawn_sibling(&Path) exactly as designed: three Stdio::null() slots, inherited environment, spawn, immediate drop of the Child, Result<(), String> with the attempted path embedded in the failure message; add pub mod spawn; to src/app.rs.
2. Step 2 — grow the action vocabulary: add the OpenWorkspace variant to HeaderAction in src/ui/layout.rs; the compiler will flag the exhaustive match in src/app/root.rs — fix nothing there yet, carrying the temporary breakage intentionally until Step 4 so the reviewer sees the arm was forced, or add a placeholder arm immediately; either way no other match sites exist to reconcile (verify with rg 'HeaderAction::' src).
3. Step 3 — wire the menu: in the layout.rs Workspace menu_button, insert ("Open workspace", HeaderAction::OpenWorkspace) into the tuple array directly before the Disconnect row, reusing the existing if ui.button(label).clicked() { s.on_header_action(action); ui.close(); } loop unchanged, so the menu closes after the click like every sibling entry.
4. Step 4 — implement the handler: add the spawn_target_override field (Document as test-only; None in production) to PacketApp and initialize it in the Default impl beside screen: Screen::Welcome; implement open_workspace per the design (toast info on success / warning on failure, zero session mutation); add the HeaderAction::OpenWorkspace => self.open_workspace(), arm to on_header_action at root.rs ~1878, adjacent to the Disconnect arm.
5. Step 5 — unit-test the spawn module in-file (src/app/spawn.rs #[cfg(test)]): (a) spawn_sibling against an innocuous executable (e.g. /usr/bin/true or, portably, std::env::current_exe() would be WRONG — it would recursively relaunch the test binary; use an OS-present trivial program) returns Ok and the parent function returns without blocking; (b) spawn_sibling(Path::new("/nonexistent/packet-sibling-bin")) returns Err whose message contains '/nonexistent/packet-sibling-bin'; (c) resolve_self_executable() Ok-point exists on disk and is executable; (d) assert by inspection/compile that spawn_sibling performs no .env_clear (code review invariant, commented in the function doc).
6. Step 6 — UI tests in src/app/root.rs mirroring the pending_repository_refresh_does_not_block_ui_interactions pattern: with fixture(), frame(&mut app,&ctx,vec![]), then click_text 'Workspace' and click_text 'Open workspace'; with app.spawn_target_override preset to a benign path, assert a toast containing 'Opening a new Packet window' appears and, critically, the connected screen, draft text and any fake active turn are byte-unchanged after the click; repeat the click twice to prove repeat-invocation spawns again without deadlock; set the override to a missing path, click again, and assert the warning toast shows the embedded path while the window remains on its connected screen with the draft intact.
7. Step 7 — run the gates: full cargo test green on Rust 1.98, and compare a sorted cargo clippy --all-targets listing against the recorded pre-work baseline per D-34 (zero added lines, no warning naming spawn_sibling/resolve_self_executable/OpenWorkspace); confirm Cargo.toml diff is empty.

## Acceptance criteria

- Given a connected window mid-way through a planner turn (status line reads '● Working'), when the operator opens Workspace and clicks 'Open workspace', then a second Packet window appears presenting the initial screen (path field, Open button) within a couple of seconds, and the first window's turn keeps advancing with no 'Turn aborted' toast and no change to its draft or status.
- Given the same connected window, when 'Open workspace' is clicked, then the invoking window's active turn, active implementations, queue and draft are unchanged in state (observably: no cancel requests issued, screen still Connected, composer draft text identical), because the new handler touches none of those fields.
- Given an environment started with PACKET_HOME=<scratch-dir> and PACKET_PI_BIN=<custom path>, when 'Open workspace' spawns a sibling, then the sibling reads the same overridden home and harness binary (observable: its session state lands under <scratch-dir> and its harness discovery probes the custom binary), because the spawn copies no environment deliberately — it inherits wholesale.
- Given the spawn fails (simulated in tests by pointing the seam at a nonexistent binary; in practice a deleted/moved executable), when 'Open workspace' is clicked, then a warning toast appears embedding the failing binary path or IO error, the menu closed cleanly, the screen stays Connected, and no partially-started zombie interaction occurs; clicking again retries identically.
- Given two sibling windows both connected to the SAME repository, when both persist chat or contended for the implementation queue, then no corrupt interleaved file bytes appear (atomic rename-swap writes win whole) and queue access serializes through the existing filesystem lock — no crash, no lost-update corruption beyond last-whole-write-wins.

## Test plan

1. Unit, src/app/spawn.rs: spawn_sibling against a trivial OS executable (pick one guaranteed present, e.g. /usr/bin/true on the Linux test host; record the path choice in the test) returns Ok(()) promptly (<2 s, proving fire-and-forget non-blocking) and the calling thread never observes the child's state afterward.
2. Unit, src/app/spawn.rs: spawn_sibling(Path::new("/nonexistent/packet-sibling-bin")) returns Err(msg) with msg.contains("/nonexistent/packet-sibling-bin"); separately assert the function does not block (measure elapsed < 2 s) — covers the spawn-failure contract without needing a hostile filesystem.
3. Unit, src/app/spawn.rs: resolve_self_executable() returns Ok(exe) with exe.exists() and metadata permissions executable bit set; this nails the production binary-resolution path the menu handler relies on.
4. UI, src/app/root.rs (mirror the existing click_text/frame pattern): connected fixture; preset spawn_target_override to the harmless binary; click Workspace then 'Open workspace'; assert the rendered frame contains the info toast text 'Opening a new Packet window', the composer draft string is byte-equal to its pre-click value, and the screen projections (session title, '● Working' or '● Ready') are unchanged — proving the menu-to-handler wiring and the zero-side-effect contract.
5. UI negative, src/app/root.rs: preset the override to a missing path; click 'Open workspace'; assert the WARNING toast embeds the missing path substring, assert no info toast fired, assert the window is still Connected with its draft intact, and that a second click retries and fails the same way gracefully (repeat-request stability).

## Verification commands and expected evidence

1. Working directory /mnt/DevProj/Packet (package 'packet', rustc/cargo 1.98.1 verified present on this machine, matching the D-34 pin). AFTER implementation: (1) cargo test app::spawn — expected: the four new spawn-module unit tests pass, 0 failures; (2) cargo test — expected: full suite green, incumbent tests plus new, matching the D-34/NFR-8 regression bar; (3) cargo clippy --all-targets 2>&1 | grep -E '^warning' | sort > /tmp/after.txt and diff against the team-recorded pre-work sorted clippy capture — expected: zero ADDED lines and no warning naming spawn_sibling, resolve_self_executable or OpenWorkspace (baseline 110 warnings at 3ba5aa2 per D-34; cleaning pre-existing debt is NOT owed by this ticket). NOTE: these were NOT pre-run — the module does not yet exist in the tree at base ead188a; commands verified only for availability (cargo present, manifest package name 'packet', test target compiles as today). Manual E2E for the success criterion itself: launch Packet connected to any project (seeded fixture per the runbook habit), start a long turn, Workspace → 'Open workspace'; expect a second window on the initial screen while window one's '● Working' line keeps ticking; close window one and confirm window two persists.

## Edge cases and failure handling

- Rapid double-click / held-press on 'Open workspace': the menu closes after the first click (existing ui.close() behavior), but a scripted double-dispatch of the action must still produce two healthy siblings with no shared-resource collision — each sibling is a full independent process with its own eframe loop; assert no deadlock since the parent never waits on either Child.
- Spawner window is quit IMMEDIATELY after spawning: the sibling must survive (it never learns of the parent; Linux reparents it). Conversely, quitting the sibling must not disturb the original's in-flight turn — verify the original issues no cleanup tied to child lifetime (there is none; no pid registry exists by design).
- Two windows connect to the same repository concurrently: per-slug chat store under $PACKET_HOME is written by both via atomic rename-swap (whole-file visibility, NFR-2), and implementation-queue contention is owned by Project.queue_lock's existing fs lock; no new locking may be invented by this ticket.
- Non-existent or unwritable self-executable (deleted binary mid-life, unusual mount): current_exe can still succeed on Linux (/proc/self/exe survives unlink), but spawn can fail with EACCES/ENOENT — must surface as the warning toast with the path, never a panic, never a hang; the connected window remains fully usable.
- Platform note (assumption, unverified): on Windows the child may flash a console; Linux x86_64 is the D-11 target and the only platform the design promises. If Windows support is later demanded, the spawn site is the single hook point (creation-flag tweak) and the rest of the design is platform-neutral std API.

## Constraints

- Clone and repository probing use the system git CLI with vector arguments (no shell interpolation), consistent with D-03 and NFR-5

## Out of scope

- Remembered workspaces / a recents list of previously opened workspaces (declined by operator in interview)

## Rollout and compatibility

Pure additive change: one menu row, one enum variant, one new leaf module, one small method plus a test-seam field on PacketApp; no persistence format, artifact, env-var or protocol change, so there is nothing to migrate or reinitialize — existing installations pick it up on next rebuild with zero user action. Rollback is a single-revert of the commit; no stored state depends on it, and reverting only removes the menu entry (both sibling processes, once launched, are independent binaries and continue working regardless). Deployment follows the existing from-source build flow on the operator's Linux workstation (D-11); no packaging, installer or service dimension exists to coordinate.

## Definition of done

- The four spawn-module unit tests and the two root.rs UI tests pass, and the FULL cargo test suite is green on Rust 1.98 (evidence: test run output, zero failures, incumbent count not regressed).
- Sorted-clippy delta versus the recorded pre-work baseline shows zero added lines and no warning naming a symbol introduced by this ticket (evidence: the two sorted listings plus their diff), and git diff --stat against base shows changes confined to the five named files with Cargo.toml untouched.
- Manual demonstration passed: from a connected window with a running turn, 'Open workspace' produced a second window on the initial screen; the first window's turn proceeded to completion or continued unabated (no 'Turn aborted' toast), and killing either window left the other alive and responsive.

## Instructions for the implementing model

Read this story and its linked dependencies before editing. Confirm the named existing files and interfaces; proposed files are explicitly labeled. Implement only this task's scope, preserving the constraints above. Run the verification commands and record actual results, changed files, and any remaining limitations. Do not mark complete on a build alone when acceptance criteria require runtime interaction. If an assumption contradicts the repository or approved specification, report the discrepancy rather than silently inventing a requirement.

## Decision

Implement the approved design described by the ticket and its frozen specification. The verified implementation changed:

src/app.rs
src/app/root.rs
src/ui/layout.rs

## Consequences

Correction to attempt 1's malformed report: the required top-level `summary` field is now present and the full report is reproduced; the implementation itself was verified unmodified in the worktree (uncommitted changes: src/app.rs +1, src/ui/layout.rs +2, src/app/root.rs +245, new src/app/spawn.rs; no commits by this agent; Cargo.toml/Cargo.lock byte-identical; stale prior-run dotfile removed to keep the change set clean). Delivered per ticket: the Workspace menu gains 'Open workspace' immediately above 'Disconnect' (label exact, deliberately ungated during busy turns); its arm calls PacketApp::open_workspace(), which resolves the running binary via resolve_self_executable (or the #[cfg(test)-only seam, None in shipping code), calls spawn_sibling — Stdio::null on all three streams, whole inherited environment, Child dropped immediately so the parent never holds, polls, or reaps the sibling — and pushes an info toast ('Opening a new Packet window') or a path-embedding warning toast; it mutates no screen, dialog, draft, queue, synthesis, or cancellation state. Fresh gate evidence on the pinned rustc/cargo 1.98.1: full `cargo test` rc=0 (407 lib + 2 integration passed, 0 failed, 1 opt-in ignored), `cargo test --lib app::spawn` 4/4, `cargo test --lib open_workspace` 2/2, manifest diff empty, change-set containment diff exact, no `env_clear` in spawn code, main.rs arg-blind, and clippy --all-targets strict parity versus a shadow expansion of HEAD (112==112 sorted warnings, empty diff, 0 naming any new symbol). Residual caveat, disclosed: the terminal native two-window demonstration requires a display, which this headless container lacks; every mechanism feeding it (zero-args spawn, arg-blind main, Welcome-default screen, wholesale env inheritance, no-touch contract, failure+retry path) is automated-tested.

## Verification

- `cd "$PACKET_WORKTREE" && cargo test --lib app::spawn`
- `cd "$PACKET_WORKTREE" && cargo test --lib open_workspace`
- `cd "$PACKET_WORKTREE" && cargo test --lib chat_store`
- `cd "$PACKET_WORKTREE" && cargo test`
- `cd "$PACKET_WORKTREE" && git diff --exit-code -- Cargo.toml Cargo.lock`
- `cd "$PACKET_WORKTREE" && git diff --name-only | sort > /tmp/chg003-tracked.actual && printf '%s\n' src/app.rs src/app/root.rs src/ui/layout.rs | sort > /tmp/chg003-tracked.expected && diff /tmp/chg003-tracked.actual /tmp/chg003-tracked.expected && git ls-files --others --exclude-standard | sort > /tmp/chg003-untracked.actual && printf '%s\n' src/app/spawn.rs | sort > /tmp/chg003-untracked.expected && diff /tmp/chg003-untracked.actual /tmp/chg003-untracked.expected`
- `cd "$PACKET_WORKTREE" && ! grep -vE "^[[:space:]]*(//|///|- |\*)" src/app/spawn.rs | grep -q env_clear`
- `cd "$PACKET_WORKTREE" && ! grep -q "env::args" src/main.rs`
- `set -eu
PW="$PACKET_WORKTREE"
S="$(mktemp -d /tmp/chg003-hb.XXXXXX)"
trap "rm -rf \"$S\"" EXIT
git -C "$PW" archive HEAD | tar -x -C "$S"
( cd "$S" && CARGO_TARGET_DIR="$PW/target" cargo clippy --all-targets --locked --quiet >/tmp/chg003-clippy-head.log 2>&1 )
( cd "$PW" && cargo clippy --all-targets --locked --quiet >/tmp/chg003-clippy-now.log 2>&1 )
grep "^warning" /tmp/chg003-clippy-head.log | sort > /tmp/chg003-clippy-head.sorted
grep "^warning" /tmp/chg003-clippy-now.log | sort > /tmp/chg003-clippy-now.sorted
echo "head_warnings=$(wc -l < /tmp/chg003-clippy-head.sorted) now_warnings=$(wc -l < /tmp/chg003-clippy-now.sorted)"
diff /tmp/chg003-clippy-head.sorted /tmp/chg003-clippy-now.sorted
echo CLIPPY_PARITY_OK
grep -ci "spawn_sibling\|resolve_self_executable\|OpenWorkspace\|spawn_target_override" /tmp/chg003-clippy-now.log || true`

## Acceptance evidence

- **Given a connected window mid-way through a planner turn (status line reads '● Working'), when the operator opens Workspace and clicks 'Open workspace', then a second Packet window appears presenting the initial screen (path field, Open button) within a couple of seconds, and the first window's turn keeps advancing with no 'Turn aborted' toast and no change to its draft or status.** — Covered automatably end to end except the final pixel-level window appearance: `cargo test --lib open_workspace` (2 passed, 0 failed) drives the real Workspace menu click into 'Open workspace' with a genuinely hanging turn (whose worker sleeps until canceled) and asserts the 'Opening a new Packet window' toast is painted, no 'Turn aborted' text appears, screen stays Connected, and the turn keeps its registered controller. Initial-screen landing is proven mechanically rather than visually: `src/main.rs` inspects no argv (grep for `env::args` finds nothing, asserted by a gate command), the sibling is launched with zero arguments (`Command::new(bin)` with no args in `spawn_sibling`), `PacketApp::default()` initializes `screen: Screen::Welcome` (src/app/root.rs), and the environment-inheritance unit test proves the sibling sees the parent's whole environment. Limitation stated plainly: this headless container has no display server, so the two simultaneous native windows themselves are the remaining manual mile, exercisable via the checklist from the prior run (cargo run on a display-connected host mid-turn, Workspace > 'Open workspace', expect second window on Welcome while the first keeps 'Working'; kill either, the other survives).
- **Given the same connected window, when 'Open workspace' is clicked, then the invoking window's active turn, active implementations, queue and draft are unchanged in state (observably: no cancel requests issued, screen still Connected, composer draft text identical), because the new handler touches none of those fields.** — Met by direct assertion, not inference: `open_workspace_spawns_a_detached_sibling_without_touching_the_session` prescreens a fake in-flight state (live `TurnController` backed by a harness that hangs until canceled, an idle `Controller::idle_fixture()` in `active_implementations`, and an unsent composer draft), performs two consecutive menu clicks, and asserts after each: `Arc::ptr_eq` of the still-registered active turn, `cancel_requested() == false` on the turn, `cancellation_requested() == false` on the active implementation, `chat_draft()` byte-identical to pre-click, and `matches!(screen, Connected)`. The handler itself (`PacketApp::open_workspace`, src/app/root.rs:1297) touches only `toasts` (info/warning) and performs the spawn — no `request_cancel`, no `screen`/`dialog`/`synch`/`queue` mutation, documented in-line as the contract contrast with `disconnect()`.
- **Given an environment started with PACKET_HOME=<scratch-dir> and PACKET_PI_BIN=<custom path>, when 'Open workspace' spawns a sibling, then the sibling reads the same overridden home and harness binary (observable: its session state lands under <scratch-dir> and its harness discovery probes the custom binary), because the spawn copies no environment deliberately — it inherits wholesale.** — Proven by a real subprocess: `app::spawn::tests::spawn_sibling_inherits_the_parent_environment_wholesale` spawns a probe script THROUGH `spawn_sibling`, the child records the parent-visible variable it received to a fresh file, and the test asserts the round-tripped value equals the parent's exactly — demonstrating that variables already held by the launching process travel intact through the wholesale-inheriting environment that `PACKET_HOME`, `PACKET_TURN_TIMEOUT_SECS` and `PACKET_PI_BIN` occupy (none of which the spawn touches, reads, or re-sets). Complementing static gate: a comment-stripped grep asserts there is NO `env_clear` call site in src/app/spawn.rs (rc=0). All four spawn unit tests pass: `cargo test --lib app::spawn` rc=0.
- **Given the spawn fails (simulated in tests by pointing the seam at a nonexistent binary; in practice a deleted/moved executable), when 'Open workspace' is clicked, then a warning toast appears embedding the failing binary path or IO error, the menu closed cleanly, the screen stays Connected, and no partially-started zombie interaction occurs; clicking again retries identically.** — `open_workspace_spawn_failure_warns_with_path_and_leaves_the_session_usable` points the test seam at a nonexistent binary, clicks 'Open workspace', and asserts: a warning paints the failing-path token, no success toast fires, screen remains Connected, the draft string is intact; it then clicks a SECOND time and asserts the identical graceful failure repeats (repeat-request stability, no zombie half-interaction). Module level: `spawn_sibling_missing_binary_reports_failure_with_the_path_embedded` asserts the Err string embeds '/nonexistent/packet-sibling-bin' and the call returns in well under 2 s (no hang, no panic — the whole suite is green). Menu closure after the click is the shared `s.on_header_action(action); ui.close();` loop every sibling menu entry uses (src/ui/layout.rs).
- **Given two sibling windows both connected to the SAME repository, when both persist chat or contended for the implementation queue, then no corrupt interleaved file bytes appear (atomic rename-swap writes win whole) and queue access serializes through the existing filesystem lock — no crash, no lost-update corruption beyond last-whole-write-wins.** — This ticket adds no cross-process memory or new locking — the sibling is a separate OS process, and the gate-asserted change set is exactly {src/app.rs, src/app/root.rs, src/ui/layout.rs} + new src/app/spawn.rs, so the two same-repo defenses are the PRE-EXISTING primitives, byte-untouched here and still green: per-slug chat persistence under $PACKET_HOME writes exclusively through `crate::artifacts::atomic_write` (rename-swap; `persistence::chat_store::tests::append_then_load_round_trips` and `torn_trailing_line_is_skipped_not_fatal` pass via `cargo test --lib chat_store`, rc=0), and implementation-queue contention serializes through `implementation_queue::acquire`'s inter-process flock `try_lock` on `packet-queue.lock` (src/core/implementation_queue.rs:64) whose stock error text already anticipates this scenario: 'Auto queue is already running in another Packet window'. Whole-file last-write-wins is the established NFR-2 semantic; no weaker path is introduced.
