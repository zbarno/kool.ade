use super::super::super::*;
use super::support::*;
#[test]
fn sw_filesystem_root_has_no_up_row_but_lists_dirs() {
    let dlg = DlgBrowse::seeded(String::from("/"));
    assert!(
        dlg.rows.iter().all(|r| !r.up),
        "no up row at the filesystem root"
    );
    assert!(
        !dlg.rows.is_empty(),
        "the root still lists its subdirectories"
    );
    assert!(dlg.rows.iter().all(|r| r.path.starts_with("/")));
}

#[test]
fn sw_vanished_current_degrades_to_read_error_plus_up_row() {
    let fx = sw_fixture("ghost");
    let target = fx.join("target");
    std::fs::create_dir_all(target.join("inner")).unwrap();
    let target_c = std::fs::canonicalize(&target).unwrap();

    let mut dlg = DlgBrowse::seeded(target.to_string_lossy().into_owned());
    assert_eq!(dlg.current, target_c);

    // the browsed folder ceases to exist behind the open browser
    std::fs::remove_dir_all(&target).unwrap();
    assert!(!dlg.current.exists());
    dlg.refresh_rows();

    assert!(dlg.read_error.is_some(), "operator-visible note surfaced");
    assert!(
        dlg.rows.iter().all(|r| r.up),
        "phantom rows purged, up row remains: {:?}",
        sw_stems(&dlg.rows)
    );
    // choose-folder eligibility predicate: vanished selection -> disable
    assert!(!dlg.selection().exists());

    // climbing out lands in a readable dir and clears the note
    let up = dlg
        .rows
        .iter()
        .find(|r| r.up)
        .expect("up row offered")
        .path
        .clone();
    dlg.descend_into(up);
    assert!(dlg.read_error.is_none());
    assert_eq!(dlg.current, std::fs::canonicalize(&fx).unwrap());

    let _ = std::fs::remove_dir_all(&fx);
}

#[test]
fn sw_modal_open_on_first_frame_escape_dismisses_without_chosing() {
    let fx = sw_fixture("esc");
    std::fs::create_dir_all(fx.join("aa")).unwrap();
    let fx_c = std::fs::canonicalize(&fx).unwrap();
    let mut dlg = DlgBrowse::seeded(fx.to_string_lossy().into_owned());
    let ctx = egui::Context::default();
    sw_warm(&ctx, &mut dlg); // first pass of a fresh context is placeholders only

    // idle frame: nothing pressed — modal renders open
    let (_out, closed, choose, cancel) =
        sw_run_frame(&ctx, &mut dlg, sw_frame(1280.0, 800.0, Vec::new(), None));
    assert!(!closed, "modal stays open on its first frame");
    assert!(!choose && !cancel, "idle frame reports no buttons");

    // next frame: Escape — dismissed, nothing chosen
    let (_out, closed, choose, cancel) = sw_run_frame(
        &ctx,
        &mut dlg,
        sw_frame(
            1280.0,
            800.0,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                modifiers: Default::default(),
                pressed: true,
                repeat: false,
            }],
            None,
        ),
    );
    assert!(closed, "Escape closes the modal");
    assert!(!choose && !cancel);
    assert_eq!(dlg.selection(), fx_c, "escape never alters the selection");

    let _ = std::fs::remove_dir_all(&fx);
}

#[test]
fn sw_single_click_selects_then_choose_reports_that_selection() {
    let fx = sw_fixture("single");
    std::fs::create_dir_all(fx.join("plainB")).unwrap();
    let repo = fx.join("repoA");
    std::fs::create_dir_all(&repo).unwrap();
    sw_git_init(&repo);
    let fx_c = std::fs::canonicalize(&fx).unwrap();
    let repo_c = std::fs::canonicalize(&repo).unwrap();

    let mut dlg = DlgBrowse::seeded(fx.to_string_lossy().into_owned());
    let ctx = egui::Context::default();
    sw_warm(&ctx, &mut dlg); // fresh-context first pass carries no real geometry

    // Primed frame locates both row labels and the Choose button (their
    // centred meshes sit comfortably inside the clickable bands; layout
    // is state-free, so measured positions survive into acting frames).
    let (out, closed, choose, cancel) =
        sw_run_frame(&ctx, &mut dlg, sw_frame(1280.0, 800.0, Vec::new(), None));
    assert!(!closed && !choose && !cancel);
    let plain_at = sw_text_pos(&out, "plainB").expect("plainB row label painted");
    let repo_at = sw_text_pos(&out, "repoA").expect("repoA row label painted");
    assert_ne!(plain_at, repo_at);
    let choose_at = sw_choose_rect(&out).center();

    // ONE click on the repoA row: selection moves there, nothing else.
    let (_o, closed, choose, cancel) = sw_run_frame(
        &ctx,
        &mut dlg,
        sw_frame(1280.0, 800.0, sw_click(repo_at), None),
    );
    assert!(!closed && !choose && !cancel);
    assert_eq!(dlg.selection(), repo_c, "single click selects");
    assert_eq!(dlg.current, fx_c, "single click does not descend");

    // Press Choose folder (measured on the primed frame — button layout
    // is state-free, so the rect holds for the acting frame).
    let (_o3, closed, choose, cancel) = sw_run_frame(
        &ctx,
        &mut dlg,
        sw_frame(1280.0, 800.0, sw_click(choose_at), None),
    );
    assert!(!closed, "choose does not dismiss through the modal hook");
    assert!(choose, "Choose folder reports the pressed action");
    assert!(!cancel);
    assert_eq!(dlg.selection(), repo_c);

    let _ = std::fs::remove_dir_all(&fx);
}

#[test]
fn sw_double_click_pair_descends_selection_follows() {
    let fx = sw_fixture("dbl");
    let repo = fx.join("repoA");
    std::fs::create_dir_all(repo.join("deep")).unwrap();
    sw_git_init(&repo);
    let fx_c = std::fs::canonicalize(&fx).unwrap();
    let repo_c = std::fs::canonicalize(&repo).unwrap();
    let deep_c = std::fs::canonicalize(repo.join("deep")).unwrap();

    let mut dlg = DlgBrowse::seeded(fx.to_string_lossy().into_owned());
    let ctx = egui::Context::default();
    sw_warm(&ctx, &mut dlg); // fresh-context first pass carries no real geometry

    // Measured view: the rooted listing exposes repoA (green).
    let (out, closed, choose, _canc) = sw_run_frame(
        &ctx,
        &mut dlg,
        sw_frame(1280.0, 800.0, Vec::new(), Some(1100.0)),
    );
    assert!(!closed && !choose);
    let hit = sw_text_pos(&out, "repoA").expect("repoA row label painted");

    // Double-click part one: selects, does not navigate.
    let (_o, closed, choose, _canc) = sw_run_frame(
        &ctx,
        &mut dlg,
        sw_frame(1280.0, 800.0, sw_click(hit), Some(1200.0)),
    );
    assert!(!closed && !choose);
    assert_eq!(dlg.current, fx_c, "a lone first click must not navigate");
    assert_eq!(dlg.selection(), repo_c, "the first click selects the row");

    // Part two, 120 ms later (well under the double-click gap): descend,
    // and the selection tracks the entered folder.
    let (_o, closed, choose, _canc) = sw_run_frame(
        &ctx,
        &mut dlg,
        sw_frame(1280.0, 800.0, sw_click(hit), Some(1200.12)),
    );
    assert!(!closed && !choose);
    assert_eq!(dlg.current, repo_c, "paired second click descended");
    assert_eq!(
        dlg.selection(),
        repo_c,
        "the entered path became the selection"
    );

    // Travel the pointer out of the list, then measure the descended
    // view: a '..' row points straight back at the enclosing folder and
    // `deep` is listed.
    let park = egui::Event::PointerMoved(egui::pos2(8.0, 8.0));
    let (_o, closed, choose, _canc) = sw_run_frame(
        &ctx,
        &mut dlg,
        sw_frame(1280.0, 800.0, vec![park], Some(1300.0)),
    );
    assert!(!closed && !choose);
    let (out, closed, choose, _canc) = sw_run_frame(
        &ctx,
        &mut dlg,
        sw_frame(1280.0, 800.0, Vec::new(), Some(1400.0)),
    );
    assert!(!closed && !choose);
    let up = dlg
        .rows
        .iter()
        .find(|r| r.up)
        .expect("up row present below root");
    assert_eq!(up.path, fx_c, "up row points back at the enclosing folder");
    let dh = sw_text_pos(&out, "deep").expect("deep row label painted");

    // Nested descend: double-click `deep`; state checks only thereafter
    // (no further shape probes — the listing is verified via fields).
    sw_run_frame(
        &ctx,
        &mut dlg,
        sw_frame(1280.0, 800.0, sw_click(dh), Some(1500.0)),
    );
    sw_run_frame(
        &ctx,
        &mut dlg,
        sw_frame(1280.0, 800.0, sw_click(dh), Some(1500.12)),
    );
    assert_eq!(dlg.current, deep_c, "nested double-click descended");
    assert_eq!(
        dlg.selection(),
        deep_c,
        "selection tracked the nested descent"
    );
    let up = dlg
        .rows
        .iter()
        .find(|r| r.up)
        .expect("up row present in deep");
    assert_eq!(
        up.path, repo_c,
        "up row walks back to the enclosing work tree"
    );

    let _ = std::fs::remove_dir_all(&fx);
}
