use super::super::super::*;
use super::support::*;
use std::path::Path;
#[test]
fn sw_seed_resolution_falls_back_stepwise() {
    let fx = sw_fixture("seeds");
    let adir = fx.join("adir");
    std::fs::create_dir_all(&adir).unwrap();
    let loose = fx.join("notes.txt");
    std::fs::write(&loose, "loose file").unwrap();
    let home = fx.join("home");
    std::fs::create_dir_all(home.join("work").join("proj")).unwrap();
    let proj = home.join("work").join("proj");

    // existing dir -> canonical self
    let (cur, sel) = resolve_seed(&adir.to_string_lossy(), Some(&home), Path::new("/"));
    assert_eq!(cur, std::fs::canonicalize(&adir).unwrap());
    assert_eq!(sel, cur);

    // lone file -> its parent dir
    let (cur, sel) = resolve_seed(&loose.to_string_lossy(), Some(&home), Path::new("/"));
    assert_eq!(cur, std::fs::canonicalize(&fx).unwrap());
    assert_eq!(sel, cur);

    // tilde expands against the given home
    let (cur, sel) = resolve_seed("~/work/proj", Some(&home), Path::new("/"));
    assert_eq!(cur, std::fs::canonicalize(&proj).unwrap());
    assert_eq!(sel, cur);

    // nonsense -> $HOME (also: blank seeds take the same path)
    let ph = std::fs::canonicalize(&home).unwrap();
    let (cur, sel) = resolve_seed("/definitely-not-a-real-dir-zz", Some(&home), Path::new("/"));
    assert_eq!(cur, ph);
    assert_eq!(sel, cur);
    let (blank_cur, _) = resolve_seed("   ", Some(&home), Path::new("/"));
    assert_eq!(blank_cur, ph);

    // no home configured -> filesystem root
    let (cur, sel) = resolve_seed("/definitely-not-a-real-dir-zz", None, Path::new("/"));
    assert_eq!(cur, std::fs::canonicalize("/").unwrap());
    assert_eq!(sel, cur);

    let _ = std::fs::remove_dir_all(&fx);
}

#[test]
fn sw_listing_sorts_case_insensitive_keeps_dotdirs_excludes_files() {
    let fx = sw_fixture("list");
    for d in [".github", "beta", "Alpha", "alpha"] {
        std::fs::create_dir_all(fx.join(d)).unwrap();
    }
    std::fs::write(fx.join("README.md"), "doc").unwrap();
    std::fs::write(fx.join(".DS_Store"), "junk").unwrap();

    let dlg = DlgBrowse::seeded(fx.to_string_lossy().into_owned());
    let fx_c = std::fs::canonicalize(&fx).unwrap();
    assert_eq!(dlg.current, fx_c);
    // selection defaults to the browsed dir -> valid on frame 1
    assert_eq!(dlg.selection(), fx_c);

    let got = sw_stems(&dlg.rows);
    assert!(
        matches!(got.first().map(String::as_str), Some("..")),
        "up row leads: {got:?}"
    );
    let rest: Vec<_> = got.into_iter().skip(1).collect();
    // case-insensitive alpha order with raw-name tie-break; dot-dirs
    // kept; regular files excluded
    assert_eq!(rest, vec![".github", "Alpha", "alpha", "beta"]);

    let _ = std::fs::remove_dir_all(&fx);
}

#[test]
fn sw_git_marking_and_down_up_navigation() {
    let fx = sw_fixture("nav");
    std::fs::create_dir_all(fx.join("plainB")).unwrap();
    let repo = fx.join("repoA");
    std::fs::create_dir_all(repo.join("nested")).unwrap();
    sw_git_init(&repo);

    let fx_c = std::fs::canonicalize(&fx).unwrap();
    let repo_c = std::fs::canonicalize(&repo).unwrap();

    let mut dlg = DlgBrowse::seeded(fx.to_string_lossy().into_owned());
    let both = [String::from("plainB"), String::from("repoA")];
    assert_eq!(sw_stems(&dlg.rows)[1..], both[..], "both dirs listed once");
    // only the true working tree earns the badge
    let by_flag: Vec<_> = dlg
        .rows
        .iter()
        .filter(|r| !r.up)
        .map(|r| (sw_name_of(r), r.git))
        .collect();
    assert!(
        by_flag.contains(&(String::from("repoA"), true)),
        "repoA flagged: {by_flag:?}"
    );
    assert!(
        by_flag.contains(&(String::from("plainB"), false)),
        "plainB unflagged: {by_flag:?}"
    );

    // descend into the work tree — selection follows (spec §5); the
    // dot-dir `.git` itself is listed (kept, dot-dirs are visible) but
    // inherits the badge
    dlg.descend_into(repo_c.clone());
    assert_eq!(dlg.current, repo_c);
    assert_eq!(dlg.selection(), repo_c);
    assert!(
        sw_stems(&dlg.rows)[1..] == [".git", "nested"]
            || sw_stems(&dlg.rows)[1..] == ["nested", ".git"],
        "unexpected rows after descending: {:?}",
        sw_stems(&dlg.rows)
    );
    // everything visible below a work tree carries the badge too
    assert!(dlg.rows.iter().all(|r| r.up || r.git));

    // descend once more, then climb back two levels via up-lands
    let nested_c = std::fs::canonicalize(repo.join("nested")).unwrap();
    dlg.descend_into(nested_c);
    let up = dlg
        .rows
        .iter()
        .find(|r| r.up)
        .expect("up row present below /")
        .path
        .clone();
    assert_eq!(up, repo_c);
    dlg.descend_into(up.clone());
    assert_eq!(dlg.current, repo_c);
    dlg.ascend();
    assert_eq!(dlg.current, fx_c);
    assert_eq!(dlg.selection(), fx_c);
    // revisit: cached verdicts give the identical listing
    let flagged: Vec<bool> = dlg.rows.iter().map(|r| r.git).collect();
    assert_eq!(flagged, vec![false, false, true]);

    let _ = std::fs::remove_dir_all(&fx);
}
