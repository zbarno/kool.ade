use super::super::*;
use std::path::{Path, PathBuf};
#[test]
fn parse_github_url_accepts_canonical_forms() {
    let cases = [
        (
            "https://github.com/octocat/hello-world",
            "octocat",
            "hello-world",
        ),
        (
            "https://github.com/octocat/hello-world.git",
            "octocat",
            "hello-world",
        ),
        (
            "https://github.com/octocat/hello-world/",
            "octocat",
            "hello-world",
        ),
        (
            "https://github.com/octocat/hello-world.git/",
            "octocat",
            "hello-world",
        ),
        (
            "  https://github.com/octocat/hello-world  ",
            "octocat",
            "hello-world",
        ),
    ];
    for (raw, owner, repo) in cases {
        let t = parse_github_url(raw).unwrap_or_else(|e| panic!("{raw:?} was rejected: {e}"));
        assert_eq!(t.owner, owner, "{raw:?}");
        assert_eq!(t.repo, repo, "{raw:?}");
        assert_eq!(
            t.url, "https://github.com/octocat/hello-world.git",
            "rebuilt canonical: {raw:?}"
        );
    }
}

#[test]
fn parse_github_url_preserves_segment_case_and_rebuilds_canonical() {
    let t = parse_github_url("HTTPS://GITHUB.COM/Octo/Repo.GIT").expect("mixed-case accepted");
    assert_eq!(t.owner, "Octo", "original segment case kept");
    assert_eq!(t.repo, "Repo");
    assert_eq!(t.url, "https://github.com/Octo/Repo.git");

    let t = parse_github_url("HtTpS://GiThUb.CoM/A.B-C_D/x_y.z-9").expect("accepted");
    assert_eq!(t.owner, "A.B-C_D");
    assert_eq!(t.repo, "x_y.z-9");
    assert_eq!(t.url, "https://github.com/A.B-C_D/x_y.z-9.git");
}

#[test]
fn parse_github_url_rejects_each_quirk_with_distinct_actionable_guidance() {
    const SHAPE: &str = "https://github.com/{owner}/{repo}";
    let rejected: &[(&str, &str)] = &[
        ("", "e.g. https://github.com/octocat/hello-world"),
        ("   ", "e.g. https://github.com/octocat/hello-world"),
        ("notaurl", "e.g. https://github.com/octocat/hello-world"),
        ("ftp://github.com/o/r", "got scheme 'ftp'"),
        ("http://github.com/o/r", "got scheme 'http'"),
        ("ssh://git@github.com/o/r", "got scheme 'ssh'"),
        ("git@github.com:o/r.git", "SSH form"),
        ("https://gitee.com/o/r", "got gitee.com"),
        ("https://githubcorp.com/o/r", "got githubcorp.com"),
        ("https://user:token@github.com/o/r", "login"),
        ("https://github.com:443/o/r", "port number"),
        ("https://github.com/o/r?ref=1", "extras"),
        ("https://github.com/o/r#anchor", "extras"),
        ("https://github.com/o", "owner alone"),
        ("https://github.com", "owner alone"),
        ("https://github.com/o/r/branch", "Too many parts"),
        ("https://github.com/o/r//", "Too many parts"),
        (
            "https://github.com/../r",
            "start and end with a letter or digit",
        ),
        (
            "https://github.com/o/..",
            "start and end with a letter or digit",
        ),
        (
            "https://github.com/lead-/r",
            "start and end with a letter or digit",
        ),
        (
            "https://github.com/o/trail-",
            "start and end with a letter or digit",
        ),
        (
            "https://github.com/caf\u{e9}/r",
            "start and end with a letter or digit",
        ),
        (
            "https://github.com/./r",
            "start and end with a letter or digit",
        ),
        ("https:///o/r", "looked like a host"),
    ];
    for (raw, needle) in rejected {
        let err = parse_github_url(raw)
            .err()
            .unwrap_or_else(|| panic!("{raw:?} was accepted"));
        assert!(
            err.contains(needle),
            "{raw:?}: `{err}` lacks guidance `{needle}`"
        );
        assert!(
            err.contains(SHAPE) || err.contains("Paste a github.com URL"),
            "{raw:?}: `{err}` does not name the canonical shape"
        );
    }
}

// ---- clone_destination -------------------------------------------------

#[test]
fn clone_destination_joins_home_and_repo_verbatim() {
    assert_eq!(
        clone_destination("My-Repo", Path::new("/Users/op")),
        PathBuf::from("/Users/op/kool-ade-workspaces/My-Repo")
    );
    assert_eq!(
        clone_destination("a.b-c_d", Path::new("/srv/repos")),
        PathBuf::from("/srv/repos/kool-ade-workspaces/a.b-c_d")
    );
}
