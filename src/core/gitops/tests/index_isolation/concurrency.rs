use super::*;

#[test]
fn parallel_koolade_commits_serialize_on_the_latest_head() {
    let repo = seed_repo("parallel");
    let first = repo.clone();
    let first_thread = std::thread::spawn(move || {
        fs::write(first.join("planning/first.md"), "first\n").unwrap();
        commit(&first, "planner: first", &["planning/first.md".into()]).unwrap()
    });
    let second = repo.clone();
    let second_thread = std::thread::spawn(move || {
        fs::write(second.join("planning/second.md"), "second\n").unwrap();
        commit(&second, "planner: second", &["planning/second.md".into()]).unwrap()
    });
    assert!(!first_thread.join().unwrap().is_empty());
    assert!(!second_thread.join().unwrap().is_empty());
    assert_eq!(
        git_ok(&repo, &["show", "HEAD:planning/first.md"]),
        "first\n"
    );
    assert_eq!(
        git_ok(&repo, &["show", "HEAD:planning/second.md"]),
        "second\n"
    );
    let _ = fs::remove_dir_all(repo);
}
