RESUME / INTEGRATION-CORRECTION PASS. State found: the full CHG-003 browse implementation (96x42 'Browse…' button beside the initial-screen path field in welcome::paint with the browse_requested out-param; DlgBrowse/DirRow/resolve_seed/refresh_rows/paint_browse_card in src/app/dialogs.rs; Dialog::Browse variant pushed from the Screen::Welcome arm and hosted in render_dialog via overlays::show_modal titled 'Choose a workspace folder'; eleven sw_ regression tests; task ADR) was already in place and staged. Root cause of the failed integration verification ('/bin/sh failed' with EMPTY stdout/stderr) was localized to the previous report's verification commands, not the implementation: the scope gate ran `git diff --name-only` (worktree-vs-INDEX), which is empty while the changes sit staged, and expected exactly the three source files while ignoring this task's mandatory ADR report artifact, so its final `test "$files" = expected` exited 1 with no diagnostic text — matching the harness's empty capture — and the clippy gate leaned on a hard-coded line cap instead of a pristine diff. Remediations: (1) all six reported verification commands rewritten to be state-robust (staged or unstaged), strict POSIX (no bashisms), and loud on failure (log tails on every failing branch); the scope gate now compares `git diff --name-only HEAD` plus untracked files against the allowlist (three source files + this ADR) and asserts zero Cargo.toml/Cargo.lock drift; (2) the clippy gate recomputes the pristine-HEAD baseline hermetically (`git archive HEAD` into a throwaway tree, cached per commit SHA in /tmp), proves ZERO added warning kinds via a `comm` diff of the sorted lists, and greps the current log for any browse-symbol citation; (3) removed a welcome::paint hover-repaint wart (browse_hovered/request_repaint block forcing a continuous repaint loop while the pointer rested on the Browse button — no functional role) found in review. Re-executed all six commands VERBATIM through /bin/sh with PACKET_WORKTREE exported: targeted app::welcome/app::dialogs/sw_ tests 34/34; full offline suite green (423 lib + main + migrate_chg003_batch + tests/multi_repository_feature.rs + tests/task_workflow.rs + doc-tests); clippy 111 warning lines after vs 112 pristine-HEAD (one pre-existing line eliminated, zero added, zero citing DlgBrowse/paint_browse_card/browse_requested/resolve_seed/refresh_rows/descend_into/Browse); diff scope exactly src/app/{welcome,dialogs,root}.rs + the task ADR, no manifest change. The ticket's interactive 'cargo run' smoke is realized by frame-driven tests driving the PRODUCTION welcome::paint and PacketApp::render_dialog through egui Context::run_ui (automation host has no interactive display); this substitution is recorded in the ADR.

Ticket: `planning/tasks/CHG-003-switch-workspaces/CHG-003-TASK-pick-an-existing-local-git-working-tree-with-a-browse-button-beside-the-initial-screen-path-field.md`

## Acceptance criteria

- Given the initial screen with any text in the path field, When I click 'Browse…', Then the modal titled 'Choose a workspace folder' opens over an input-blocking backdrop with a legend line and a list of subdirectories of the seeded directory; cancelling via X, Escape or 'Cancel' leaves conn_path byte-identical, shows no toast, and attempts no connect.: Executed and green under /bin/sh (verification cmd 1; 34/34). app::root::tests::sw_welcome_browse_button_signals_request_when_clicked locates the 96x42 'Browse…' rect painted beside the path field by welcome::paint and a press/release on its centre raises the one-shot browse_requested flag, which the Screen::Welcome arm converts into Dialog::Browse(DlgBrowse::seeded(conn_path)). app::dialogs::tests::sw_modal_open_on_first_frame_escape_dismisses_without_chosing renders the real overlays::show_modal chrome (title 'Choose a workspace folder', blocking foreground panel, legend line, seeded subdirectory rows) with frame 1 closed=false and Escape closed=true, no choose/cancel side effect. All three dismissal routes (Cancel by label, modal close ✕ by panel-corner rect, Escape key event) are driven through the PRODUCTION PacketApp::render_dialog in app::root::tests::sw_browse_choice_writes_conn_path_only_then_existing_submit_connects: each consumes the dialog, leaves conn_path and conn_error byte-identical, stays on Screen::Welcome, and never reaches submit_connect (the sole producer of toasts and connects), so no toast and no connect can occur.
- Given a machine where /tmp/swtest/repoA is an initialized git tree, When I open Browse, descend into repoA, single-click a row, and press 'Choose folder', Then the modal closes, conn_path equals the canonical absolute path of that directory, the screen is still the initial screen with no toast, and a subsequent Open/Enter runs the existing submit_connect and connects normally.: Executed and green under /bin/sh (verification cmds 1 and 6). app::root::tests::sw_browse_choice_writes_conn_path_only_then_existing_submit_connects builds a git-initialized working-tree fixture (the /tmp/swtest/repoA analogue), seeds the browser from the field, single-clicks the tree's row, presses 'Choose folder': the dialog is consumed, conn_path equals fs::canonicalize(fixture) verbatim, the screen is still Welcome, conn_error byte-untouched (no toast, no connect). The pre-existing app.submit_connect() (Open/Enter authority, unchanged) then lands Screen::Connected titled 'site-app' with conn_error None and chat state persisted under an isolated PACKET_HOME, proving the full existing connect pipeline runs after a browse choice. Descent mechanics (double-click pairing, selection tracking, up-row) are separately pinned by app::dialogs::tests::sw_double_click_pair_descends_selection_follows and sw_single_click_selects_then_choose_reports_that_selection.
- Given a seeded directory containing 'aa', 'BB', '.hidden' subdirectories, one loose file, and one uninitialized 'plainB' folder, When the browser lists it, Then rows appear in case-insensitive name order excluding the loose file (plus the leading up row where applicable), '.hidden' is included, and exactly the rows inside a git working tree render in theme::SUCCESS while others render in theme::TEXT.: Executed and green under /bin/sh (verification cmd 1). app::dialogs::tests::sw_listing_sorts_case_insensitive_keeps_dotdirs_excludes_files pins the leading up row plus the exact case-insensitive stem list (['.github','Alpha','alpha','beta'] on a fixture carrying dot-dir, mixed-case ties, and a loose README.md that is excluded); sw_git_marking_and_down_up_navigation pins the marks with the REAL gitops::is_work_tree probe (repoA true, its descendant true, non-git sibling plainB false — rev-parse walks upward, so descendants inherit the badge by design). paint_browse_card (src/app/dialogs.rs) colours each selectable row theme::SUCCESS iff row.git else theme::TEXT, so exactly the working-tree rows render green; the flag is computed once per navigation and memoized in git_cache, never during paint, and nothing is pre-judged (git-flag-blind write-back).
- Given the browser pointed at a directory that is NOT a git working tree, When I choose it, Then the path is inserted identically to a git tree (validation stays with attempt_connect): pressing Open afterwards reproduces the existing 'no .git directory found' InvalidRepo banner in the card's red box — the browser neither enables nor disables that path.: Executed and green under /bin/sh (verification cmd 6, --exact named test). app::root::tests::sw_browse_choice_on_plain_folder_then_open_shows_legacy_invalid_repo_banner chooses a non-git folder through the production dialog: conn_path receives the identical canonical absolute path (lossy String, git-flag-blind d.selection() write-back), the screen stays Welcome with no connect; the UNCHANGED submit_connect then refuses the folder and fills conn_error with the banner containing 'no .git directory found' naming the chosen path. git diff HEAD shows zero edits to welcome::attempt_connect or submit_connect.
- Given the current folder ceases to exist while the modal is open, When I navigate or repaint, Then the list area shows a dim 'cannot read' notice with the OS error text, the up row (if any) still works, and 'Choose folder' is disabled while the remembered selection no longer exists — no panic, no crash.: Executed and green under /bin/sh (verification cmd 1). app::dialogs::tests::sw_vanished_current_degrades_to_read_error_plus_up_row deletes the browsed folder behind the live browser: refresh_rows captures the OS error into read_error (painted as the dim 'Cannot read this folder: {error}' line; paint_browse_card re-degrades on repaint via the !current.is_dir() liveness probe), phantom rows are purged leaving the up row, selection().exists()==false so the can_choose = selected.exists() && selected.is_dir() predicate disables 'Choose folder', and ascending via the up row lands a readable directory and clears read_error. The full offline suite finishing green (423 lib, 0 failed) demonstrates no panic/crash path.
- Given a pristine start where conn_path defaulted to the app's current working directory (root.rs:111), When I open Browse, Then it seeds from that directory; if the seed path is gone it falls back to $HOME, and if HOME is unset to '/' — every case still presents at least the up-less root listing and an immediately selectable current directory.: Executed and green under /bin/sh (verification cmd 1). app::dialogs::tests::sw_seed_resolution_falls_back_stepwise drives the PURE resolve_seed(seed, home, root) through the whole ladder with zero environment mutation: existing dir -> (canon, canon); lone file -> canonical parent; '~/..' expanded against the given home; bogus and blank seeds -> canonical $HOME; home=None -> canonical '/' — every outcome (existing_directory, the_same), so Choose is valid from frame one. Production DlgBrowse::seeded wires std::env::var_os('HOME') with root '/', and the Welcome arm seeds from the CURRENT field contents (the root.rs:111 current_dir() default is untouched — see diff). sw_filesystem_root_has_no_up_row_but_lists_dirs proves even the '/' fallback presents an up-less listing of real subdirectories with the current dir immediately selected.

## Validation

Packet reran these commands successfully in the implementation worktree:

```sh
cd "$PACKET_WORKTREE" || exit 1
cargo test --offline --lib -- app::welcome app::dialogs sw_ > /tmp/pv_chg003_1.log 2>&1
rc=$?
tail -3 /tmp/pv_chg003_1.log
if [ "$rc" -eq 0 ] && grep -q "34 passed; 0 failed" /tmp/pv_chg003_1.log \
   && grep -q "connecting_missing_path_errors_friendly" /tmp/pv_chg003_1.log \
   && grep -q "connecting_nonexistent_path_errors_friendly" /tmp/pv_chg003_1.log \
   && grep -q "sw_browse_choice_on_plain_folder_then_open_shows_legacy_invalid_repo_banner" /tmp/pv_chg003_1.log; then
  echo TARGETED_TESTS_OK
else
  tail -40 /tmp/pv_chg003_1.log
  exit 1
fi
```

```sh
cd "$PACKET_WORKTREE" || exit 1
cargo test --offline > /tmp/pv_chg003_2.log 2>&1
rc=$?
grep -E "test result" /tmp/pv_chg003_2.log
if [ "$rc" -eq 0 ] && grep -q "Running tests/multi_repository_feature.rs" /tmp/pv_chg003_2.log \
   && grep -q "Running tests/task_workflow.rs" /tmp/pv_chg003_2.log \
   && [ "$(grep -c "test result: ok" /tmp/pv_chg003_2.log)" -ge 6 ] \
   && ! grep -Eq "^test .*(FAILED|panicked)" /tmp/pv_chg003_2.log; then
  echo FULL_OFFLINE_SUITE_OK
else
  tail -40 /tmp/pv_chg003_2.log
  exit 1
fi
```

```sh
cd "$PACKET_WORKTREE" || exit 1
head_sha=$(git rev-parse HEAD) || exit 1
base_list="/tmp/pv_chg003_clippy_head_${head_sha}.txt"
if [ ! -f "$base_list" ]; then
  base_ws="/tmp/pv_chg003_clippysrc_${head_sha}"
  rm -rf "$base_ws" && mkdir -p "$base_ws" || exit 1
  git archive HEAD | tar -x -C "$base_ws" || exit 1
  ( cd "$base_ws" && cargo clippy --offline --all-targets --quiet > /tmp/pv_chg003_3_pris.log 2>&1 )
  pc=$?
  if [ "$pc" -ne 0 ]; then
    echo "pristine-head clippy failed to build (unexpected)"
    tail -20 /tmp/pv_chg003_3_pris.log
    exit 1
  fi
  grep -E '^warning' /tmp/pv_chg003_3_pris.log | sort > "$base_list" || true
fi
cargo clippy --offline --all-targets --quiet > /tmp/pv_chg003_3.log 2>&1
cc=$?
if [ "$cc" -ne 0 ]; then
  tail -20 /tmp/pv_chg003_3.log
  exit 1
fi
grep -E '^warning' /tmp/pv_chg003_3.log | sort > /tmp/pv_chg003_3_after.txt || true
echo "-- warning-line counts (pristine-head baseline, after-change) --"
wc -l "$base_list" /tmp/pv_chg003_3_after.txt
sort -u "$base_list" > /tmp/pv_chg003_3_base_u.txt
sort -u /tmp/pv_chg003_3_after.txt > /tmp/pv_chg003_3_after_u.txt
comm -13 /tmp/pv_chg003_3_base_u.txt /tmp/pv_chg003_3_after_u.txt > /tmp/pv_chg003_3_added.txt
if [ -s /tmp/pv_chg003_3_added.txt ]; then
  echo "NEW clippy warning kinds added by the change:"
  cat /tmp/pv_chg003_3_added.txt
  exit 1
fi
if grep -En 'DlgBrowse|paint_browse_card|browse_requested|resolve_seed|refresh_rows|descend_into|Browse' /tmp/pv_chg003_3.log; then
  echo "FAIL: clippy cites a browse-related symbol"
  exit 1
fi
echo CLIPPY_ZERO_ADDED_WARNINGS_NO_BROWSE_SYMBOL_CITES
```

```sh
cd "$PACKET_WORKTREE" || exit 1
{ git diff --name-only HEAD; git ls-files --others --exclude-standard; } > /tmp/pv_chg003_4_raw.txt || exit 1
sed '/^[[:space:]]*$/d' /tmp/pv_chg003_4_raw.txt | LC_ALL=C sort -u > /tmp/pv_chg003_4_actual.txt
echo "-- changed/new files versus HEAD (staged or unstaged) plus untracked --"
cat /tmp/pv_chg003_4_actual.txt
printf '%s\n' \
  "adr/implement-chg-003-task-pick-an-existing-local-git-working-tree-wit.md" \
  "src/app/dialogs.rs" \
  "src/app/root.rs" \
  "src/app/welcome.rs" | LC_ALL=C sort > /tmp/pv_chg003_4_expect.txt
if diff /tmp/pv_chg003_4_actual.txt /tmp/pv_chg003_4_expect.txt \
   && git diff --quiet HEAD -- Cargo.toml Cargo.lock; then
  echo DIFF_SCOPE_THREE_SOURCE_FILES_PLUS_TASK_ADR_NO_MANIFEST_CHANGE
else
  echo "FAIL: scope drift or manifest change detected"
  exit 1
fi
```

```sh
cd "$PACKET_WORKTREE" || exit 1
for t in sw_welcome_browse_button_signals_request_when_clicked sw_browse_choice_writes_conn_path_only_then_existing_submit_connects sw_browse_choice_on_plain_folder_then_open_shows_legacy_invalid_repo_banner sw_seed_resolution_falls_back_stepwise sw_listing_sorts_case_insensitive_keeps_dotdirs_excludes_files sw_git_marking_and_down_up_navigation sw_vanished_current_degrades_to_read_error_plus_up_row sw_filesystem_root_has_no_up_row_but_lists_dirs sw_modal_open_on_first_frame_escape_dismisses_without_chosing sw_single_click_selects_then_choose_reports_that_selection sw_double_click_pair_descends_selection_follows; do
  grep -rq "fn ${t}(" src/app || { echo "FAIL: missing regression test fn ${t}"; exit 1; }
done
grep -q "fn paint(" src/app/welcome.rs || { echo "FAIL: welcome::paint missing"; exit 1; }
grep -q "browse_requested: &mut bool" src/app/welcome.rs || { echo "FAIL: browse_requested out-param missing"; exit 1; }
grep -q "Browse" src/app/welcome.rs || { echo "FAIL: Browse button label missing"; exit 1; }
grep -q "Browse(DlgBrowse)" src/app/root.rs || { echo "FAIL: Dialog::Browse variant missing"; exit 1; }
grep -q "DlgBrowse::seeded(self.conn_path" src/app/root.rs || { echo "FAIL: Welcome-arm push missing"; exit 1; }
grep -q "Choose a workspace folder" src/app/root.rs || { echo "FAIL: modal title missing"; exit 1; }
grep -q "self.conn_path = d.selection()" src/app/root.rs || { echo "FAIL: conn_path injection missing"; exit 1; }
grep -q "pub fn paint_browse_card" src/app/dialogs.rs || { echo "FAIL: paint_browse_card missing"; exit 1; }
grep -q "pub(crate) fn resolve_seed" src/app/dialogs.rs || { echo "FAIL: resolve_seed missing"; exit 1; }
grep -q "pub fn seeded" src/app/dialogs.rs || { echo "FAIL: DlgBrowse::seeded missing"; exit 1; }
echo WIRING_AND_ALL_REGRESSION_TESTS_PRESENT
```

```sh
cd "$PACKET_WORKTREE" || exit 1
cargo test --offline --lib -- app::root::tests::sw_browse_choice_writes_conn_path_only_then_existing_submit_connects app::root::tests::sw_browse_choice_on_plain_folder_then_open_shows_legacy_invalid_repo_banner --exact > /tmp/pv_chg003_6.log 2>&1
rc=$?
grep -E "test app::root::tests|test result" /tmp/pv_chg003_6.log
if [ "$rc" -eq 0 ] && grep -q "2 passed; 0 failed" /tmp/pv_chg003_6.log; then
  echo CONNECT_AUTHORITY_PROOF_OK
else
  tail -30 /tmp/pv_chg003_6.log
  exit 1
fi
```

