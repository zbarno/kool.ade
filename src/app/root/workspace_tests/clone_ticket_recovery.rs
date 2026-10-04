use super::*;

#[test]
fn sw_clone_ticket_one_unsettled_worker_rides_back_next_tick() {
    let (join, gate) = gated_worker(|| -> Result<std::path::PathBuf, crate::error::AppError> {
        Err(crate::error::AppError::Other(
            "abandoned at teardown".into(),
        ))
    });
    let mut app = KooladeApp {
        conn_github: "https://github.com/acme/site".into(),
        ..Default::default()
    };
    app.clone_job = Some(sw_cl_job(join));
    let ctx = egui::Context::default();

    // Worker still fetching: the ticket picks the job UP and puts it
    // back (poll rhythm), and NO action is taken meanwhile.
    app.tick(0.016, &ctx);
    assert!(app.clone_job.is_some(), "unsettled worker rides back");
    assert!(
        matches!(app.screen, Screen::Welcome),
        "no navigation while in flight"
    );
    assert!(app.conn_error.is_none(), "no premature error");
    assert_eq!(
        app.conn_github, "https://github.com/acme/site",
        "field preserved in flight"
    );

    // Second ticket while still unsettled: same behaviour repeats.
    app.tick(0.016, &ctx);
    assert!(app.clone_job.is_some(), "second tick: still riding back");

    // Gate release (late) so the thread parks out harmlessly.
    let _ = gate.send(());
}

#[test]
fn sw_clone_ticket_two_success_flows_into_submit_connect_and_connected() {
    // Serialise the ambient-env mutations (git hierarchy + per-user
    // state root) behind the house lock while a REAL connect runs.
    let _shield = crate::core::gitops::test_support::shield("sw-clone-ok");
    let state_home = std::env::temp_dir().join(format!("swcl_state_ok_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&state_home);
    std::fs::create_dir_all(&state_home).unwrap();
    // SAFETY: GIT_HIERARCHY_LOCK is held; only this test touches the
    // per-user state root during the body. The prior value (if any)
    // is restored at the end of the body.
    let prev_state_home = std::env::var_os("KOOLADE_HOME");
    unsafe {
        std::env::set_var("KOOLADE_HOME", &state_home);
    }

    let dest = swcl_repo("ok");
    let dest_for_worker = dest.clone();
    let (join, gate) = gated_worker(move || Ok(dest_for_worker));
    let mut app = KooladeApp {
        conn_github: "https://github.com/acme/site".into(),
        ..Default::default()
    };
    app.clone_job = Some(sw_cl_job(join));
    let ctx = egui::Context::default();

    app.tick(0.016, &ctx); // unsettled: rides back (proven in test one)
    gate.send(()).unwrap();
    await_settle(&app);

    // TICK TWO: job settled -> join -> refill conn_path -> submit_connect
    // (the single connect authority) -> Connected.
    app.tick(0.016, &ctx);
    assert!(app.clone_job.is_none(), "settled worker is consumed");
    let Screen::Connected(project) = &app.screen else {
        panic!(
            "expected Connected after a successful clone (err={:?})",
            app.conn_error
        );
    };
    let expected_title = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    assert_eq!(
        project.state.title, expected_title,
        "title from the repo name"
    );
    assert_eq!(
        app.conn_path,
        dest.to_string_lossy().into_owned(),
        "worker output filled conn_path (flow-through)"
    );
    assert!(app.conn_github.is_empty(), "success FORGETS the pasted URL");
    assert!(app.conn_error.is_none());
    assert!(
        dest.join(crate::artifacts::product_docs::INDEX).exists(),
        "the connect pipeline bootstrapped + migrated the cloned repo"
    );

    // Restore the ambient state root (prior value, or absence).
    unsafe {
        match prev_state_home {
            Some(prev) => std::env::set_var("KOOLADE_HOME", prev),
            None => std::env::remove_var("KOOLADE_HOME"),
        }
    }
    let _ = std::fs::remove_dir_all(&dest);
    let _ = std::fs::remove_dir_all(&state_home);
}

#[test]
fn sw_clone_ticket_three_failed_clone_raises_git_banner_and_preserves_field() {
    let (join, gate) = gated_worker(|| {
        Err(crate::error::AppError::Git {
            cmd: "clone https://github.com/acme/ghost-repo.git".into(),
            detail: "fatal: repository 'https://github.com/acme/ghost-repo.git/' not found".into(),
        })
    });
    let mut app = KooladeApp {
        conn_github: "https://github.com/acme/ghost-repo".into(),
        ..Default::default()
    };
    app.clone_job = Some(CloneJob {
        url_display: "github.com/acme/ghost-repo".into(),
        repo: "ghost-repo".into(),
        join,
    });
    let ctx = egui::Context::default();

    app.tick(0.016, &ctx);
    gate.send(()).unwrap();
    await_settle(&app);
    app.tick(0.016, &ctx);

    assert!(app.clone_job.is_none());
    assert!(
        matches!(app.screen, Screen::Welcome),
        "failure NEVER navigates"
    );
    assert_eq!(
        app.conn_github, "https://github.com/acme/ghost-repo",
        "field preserved for correction + retry"
    );
    let err = app.conn_error.as_deref().unwrap_or("");
    assert!(
        err.contains("git clone https://github.com/acme/ghost-repo.git failed"),
        "headline line rides in the banner (got: {err})"
    );
    assert!(
        err.contains("not found"),
        "detail line rides in the banner (got: {err})"
    );

    // AC retry: the failed SETTLE left the slot vacant and the field
    // intact, so a pressed Clone dispatches AGAIN — proven with a
    // second gated (network-free) worker occupying the same cycle.
    let (join2, gate2) = gated_worker(|| {
        Err(crate::error::AppError::Git {
            cmd: "clone https://github.com/acme/ghost-repo.git".into(),
            detail: "retry round: still unreachable".into(),
        })
    });
    app.clone_job = Some(CloneJob {
        url_display: "github.com/acme/ghost-repo".into(),
        repo: "ghost-repo".into(),
        join: join2,
    });
    app.tick(0.016, &ctx);
    gate2.send(()).unwrap();
    await_settle(&app);
    app.tick(0.016, &ctx);
    assert!(app.clone_job.is_none(), "second failure also settles");
    assert!(matches!(app.screen, Screen::Welcome), "still on Welcome");
    assert!(
        app.conn_error
            .as_deref()
            .is_some_and(|e| e.contains("retry round")),
        "retry-round banner rendered (got: {:?})",
        app.conn_error
    );
}
