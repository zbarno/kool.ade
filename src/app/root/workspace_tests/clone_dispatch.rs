use super::*;

#[test]
fn sw_clone_ticket_four_panicked_worker_reports_stopped_unexpectedly() {
    let (join, gate) = gated_worker(|| -> Result<std::path::PathBuf, crate::error::AppError> {
        panic!("simulated out-of-memory kill in the fetch")
    });
    let mut app = KooladeApp {
        conn_github: "https://github.com/acme/site".into(),
        ..Default::default()
    };
    app.clone_job = Some(sw_cl_job(join));
    let ctx = egui::Context::default();

    app.tick(0.016, &ctx); // rides back while unwound
    gate.send(()).unwrap();
    await_settle(&app);
    app.tick(0.016, &ctx);

    assert!(app.clone_job.is_none());
    assert!(matches!(app.screen, Screen::Welcome));
    assert_eq!(
        app.conn_error.as_deref(),
        Some("The clone worker stopped unexpectedly. Try again.")
    );
}

#[test]
fn sw_begin_clone_blank_is_noop_and_unparsable_urls_spawn_nothing() {
    let mut app = KooladeApp {
        conn_github: "   ".into(),
        ..Default::default()
    };
    app.begin_clone_from_field();
    assert!(app.clone_job.is_none(), "blank input spawns nothing");
    assert!(app.conn_error.is_none(), "blank input is a silent no-op");

    let mut app = KooladeApp {
        conn_github: "  notaurl  ".into(),
        ..Default::default()
    };
    app.begin_clone_from_field();
    assert!(
        app.clone_job.is_none(),
        "an unparseable URL spawns no worker/process"
    );
    let err = app.conn_error.as_deref().unwrap_or("");
    assert!(
        err.starts_with("Can't clone that URL"),
        "framing line (got: {err})"
    );
    assert!(
        err.contains("https://github.com/octocat/hello-world"),
        "names the canonical shape + example (got: {err})"
    );
    assert_eq!(
        app.conn_github, "  notaurl  ",
        "field untouched for correction"
    );

    // Host mismatch gets its own guidance, still no worker.
    let mut app = KooladeApp {
        conn_github: "https://gitee.com/o/r".into(),
        ..Default::default()
    };
    app.begin_clone_from_field();
    assert!(app.clone_job.is_none());
    let err = app.conn_error.as_deref().unwrap_or("");
    assert!(
        err.contains("got gitee.com"),
        "distinct host guidance (got: {err})"
    );
}

#[test]
fn sw_begin_clone_valid_url_dispatches_worker_hermetically() {
    // Proves the dispatch link (parse-success → thread launch → job
    // recorded) WITHOUT escaping the test: the computation seam stands
    // in for the real perform_clone, capturing exactly what a worker
    // would receive — the CANONICAL rebuilt url (never the raw string),
    // with the .git suffix normalized in and segment case preserved.
    use std::sync::{Arc, Mutex};
    let dest = std::env::temp_dir().join(format!("swcl_dispatch_{}_widget", std::process::id()));
    let _ = std::fs::remove_dir_all(&dest);
    std::fs::create_dir_all(&dest).unwrap();
    let captured: Arc<Mutex<(String, String)>> = Arc::new(Mutex::default());
    let cap2 = captured.clone();
    let dest_for_seam = dest.clone();
    let mut app = KooladeApp {
        conn_github: "  https://github.com/Acme/Widget.git \n".into(),
        ..Default::default()
    };
    app.clone_computation_override = Some(Arc::new(move |source: String, repo: String| {
        *cap2.lock().unwrap() = (source, repo);
        Ok(dest_for_seam.clone())
    }));

    app.begin_clone_from_field(); // trims before parsing (raw had padding)
    let Some(job) = app.clone_job.as_ref() else {
        panic!(
            "valid URL must dispatch a worker (err={:?})",
            app.conn_error
        );
    };
    assert_eq!(
        job.url_display, "github.com/Acme/Widget",
        "badge uses owner/repo"
    );
    assert_eq!(job.repo, "Widget", "segment case preserved as pasted");

    // Duplicate submission WHILE IN-FLIGHT: the occupied slot refuses
    // (defense in depth behind the busy card's input-steal) and the
    // recorded operation stays untouched. The capture below doubles as
    // proof no second worker computation ever ran.
    app.conn_github = "https://github.com/Late/Arrival".into();
    app.begin_clone_from_field();
    let job_after_dup = app
        .clone_job
        .as_ref()
        .expect("duplicate must be refused, slot intact");
    assert_eq!(job_after_dup.url_display, "github.com/Acme/Widget");
    assert!(
        app.conn_error.is_none(),
        "duplicate refused silently (no banner churn)"
    );

    let settled = app.clone_job.take().unwrap().join.join().unwrap().unwrap();
    assert_eq!(settled, dest, "worker result flows back undistorted");
    let (source, repo) = (*captured.lock().unwrap()).clone();
    assert_eq!(
        source, "https://github.com/Acme/Widget.git",
        "worker receives the CANONICAL rebuilt url (not the raw paste)"
    );
    assert_eq!(repo, "Widget");
    let _ = std::fs::remove_dir_all(&dest);
}

// ---- Connect-card paint simulation (AC7: in-flight freeze) --------
