use super::*;

pub(super) fn interrupted() -> Sandbox {
    interrupted_with_build_ignore()
}

pub(in crate::core::implementation::tests::initial_reconciliation) fn interrupted_with_configuration(
    granted: bool,
) -> Sandbox {
    interrupted_with_configuration_and_build_ignore(granted, false)
}

pub(in crate::core::implementation::tests::initial_reconciliation) fn interrupted_with_build_ignore()
-> Sandbox {
    interrupted_with_configuration_and_build_ignore(false, true)
}

fn interrupted_with_configuration_and_build_ignore(granted: bool, build_ignore: bool) -> Sandbox {
    const INSTRUCTIONS: &str = "# Instructions\n\n- Required quality gates: `mkdir -p .cache; if test -f .cache/ready; then test -f build/cache.txt; else mkdir -p build; printf cache > build/cache.txt; printf ready > .cache/ready; exit 1; fi`.\n";
    let mut initial_files = vec![("AGENTS.md", INSTRUCTIONS)];
    if build_ignore {
        initial_files.push((".gitignore", "build/\n.cache/\n"));
    }
    // These files define the fixture's starting state; avoid a throwaway commit and remote push.
    let s = Sandbox::new_with_initial_files(&initial_files);
    let (remote, local, common) = super::super::merge_recovery::make_divergent(&s);
    let dir = super::state_dir(&s.repo, &s.ticket).unwrap();
    let worktree = super::super::merge_recovery::task_worktree(&s);
    fs::create_dir_all(worktree.parent().unwrap()).unwrap();
    let branch = format!(
        "koolade/{}",
        crate::core::implementation::key_for_ticket(&s.ticket)
    );
    s.git(
        &s.repo,
        &[
            "worktree",
            "add",
            "-b",
            &branch,
            worktree.to_str().unwrap(),
            &remote,
        ],
    );
    s.git(
        &worktree,
        &["merge", "--no-ff", "--no-commit", "--no-edit", &local],
    );
    fs::create_dir_all(worktree.join(".cache")).unwrap();
    fs::write(
        worktree.join(".cache/preexisting.txt"),
        "preserve preexisting cache\n",
    )
    .unwrap();
    fs::write(
        worktree.join(".cache/.env"),
        "SYNTHETIC_CONFIG=task-cache\n",
    )
    .unwrap();
    if granted {
        fs::create_dir_all(s.repo.join(".cache")).unwrap();
        fs::write(
            s.repo.join(".cache/.env"),
            "SYNTHETIC_CONFIG=source-cache\n",
        )
        .unwrap();
        super::super::runtime_config::approve_path(&s, ".cache/.env");
    }
    super::super::save_legacy_reconciliation_state(
        &s, &s.ticket, &worktree, "main", &local, &remote, &common,
    );
    let error = super::run_with_agent(&s, &StopAfterVerification, None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("interrupted"), "{error}");
    assert!(dir.join("base-reconciliation-generated.json").exists());
    s
}
