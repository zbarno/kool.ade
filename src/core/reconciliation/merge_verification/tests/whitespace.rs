use super::*;

#[test]
// Production boundary: actual Git squash objects must preserve reviewed bytes,
// including whitespace inside code strings. A mocked patch hash cannot prove
// Git's whitespace normalization is avoided. Local Git only; low setup cost.
fn squash_changing_whitespace_inside_a_string_is_not_the_reviewed_patch() {
    let repo = Repo::new();
    let base = output(&repo.0, &["rev-parse", "HEAD"]);
    run(&repo.0, &["checkout", "-qb", "reviewed"]);
    std::fs::write(
        repo.0.join("policy.py"),
        "def message():\n    return \"access denied\"\n",
    )
    .unwrap();
    run(&repo.0, &["add", "."]);
    run(&repo.0, &["commit", "-qm", "reviewed policy"]);
    let reviewed = output(&repo.0, &["rev-parse", "HEAD"]);
    run(&repo.0, &["checkout", "-qb", "squashed", &base]);
    run(&repo.0, &["cherry-pick", "--no-commit", &reviewed]);
    std::fs::write(
        repo.0.join("policy.py"),
        "def message():\n    return \"accessdenied\"\n",
    )
    .unwrap();
    run(&repo.0, &["add", "."]);
    run(&repo.0, &["commit", "-qm", "changed policy"]);
    let merged = output(&repo.0, &["rev-parse", "HEAD"]);
    assert!(validate_merged_checkout(&repo.0, &base, &reviewed, &merged).is_err());
}
