use super::*;

#[test]
fn sw_clone_card_freezes_in_flight_and_scopes_enter_per_focus() {
    let ctx = egui::Context::default();
    let mut sim = SwCardSim {
        path: String::new(),
        github: String::new(),
        err: None,
        browse: false,
        clone_req: false,
        submitted: false,
        cloning: None,
        path_probe: (egui::Id::NULL, egui::Rect::NOTHING),
        url_probe: (egui::Id::NULL, egui::Rect::NOTHING),
    };

    // Burn the fresh context's placeholder frame, then an idle probe.
    sim.frame(&ctx, vec![]);
    let out = sim.frame(&ctx, vec![]);
    assert!(!sim.submitted, "idle card returns false");
    assert!(!sim.browse && !sim.clone_req, "idle card raises nothing");
    assert!(
        sw_text_pos(&out, "OR CLONE FROM GITHUB").is_some(),
        "row caption painted"
    );
    assert!(sw_text_pos(&out, "Clone").is_some(), "Clone button painted");
    assert!(
        sw_text_pos(&out, "Open workspace").is_some(),
        "existing Open button intact"
    );
    let (path_field, url_field) = sw_card_fields(&sim);

    // Bare window-level Enter with NO field focused: inert (retired
    // global hook).
    sim.browse = false;
    sim.clone_req = false;
    sim.submitted = false;
    sim.frame(&ctx, vec![sw_enter_event()]);
    assert!(
        !sim.submitted && !sim.clone_req && !sim.browse,
        "focus-less Enter changes nothing"
    );

    // URL field focused + Enter: requests a clone, does NOT submit.
    sim.click(&ctx, url_field.center());
    assert_eq!(
        ctx.memory(|m| m.focused()),
        Some(sim.url_probe.0),
        "click focused the URL field"
    );
    sim.clone_req = false;
    sim.submitted = false;
    sim.frame(&ctx, vec![sw_enter_event()]);
    assert!(!sim.submitted, "URL-field Enter must not submit");
    assert!(sim.clone_req, "URL-field Enter requests a clone");

    // Path field focused + Enter: submits only when non-empty.
    sim.click(&ctx, path_field.center());
    assert_eq!(
        ctx.memory(|m| m.focused()),
        Some(sim.path_probe.0),
        "click focused the PATH field"
    );
    sim.clone_req = false;
    sim.submitted = false;
    sim.frame(&ctx, vec![sw_enter_event()]);
    assert!(
        !sim.submitted && !sim.clone_req,
        "empty path + focused Enter is inert (incumbent guard)"
    );
    // Enter already SURRENDED focus (single-line TextEdit behaviour —
    // the retired global shortcut's deliberate casualty): refill and
    // re-focus before the submitting Enter.
    sim.path = "/tmp/somewhere".into();
    sim.click(&ctx, path_field.center());
    sim.submitted = false;
    sim.frame(&ctx, vec![sw_enter_event()]);
    assert!(sim.submitted, "path-field Enter submits the connect");

    // Clone button rect (110x42) is the card's unique tall outline.
    let out = sim.frame(&ctx, vec![]);
    let clone_btn = out
        .shapes
        .iter()
        .find_map(|sl| match &sl.shape {
            egui::Shape::Rect(r) => ((r.rect.size().x - 110.0).abs() < 2.01
                && (r.rect.size().y - 42.0).abs() < 2.01)
                .then_some(r.rect),
            _ => None,
        })
        .unwrap_or_else(|| panic!("Clone button rect (110x42) missing"));

    // Empty-URL guard: the control is disabled, so a click on it
    // raises nothing (ticket: mirrors the path field's empty-silence;
    // the parser's complaints belong to non-empty malformations).
    sim.clone_req = false;
    sim.submitted = false;
    sim.click(&ctx, clone_btn.center());
    assert!(
        !sim.clone_req && !sim.submitted,
        "disabled (empty-URL) Clone click raises nothing"
    );

    // Mouse path: once the field holds a URL, clicking Clone activates.
    sim.github = "https://github.com/acme/widget".into();
    sim.frame(&ctx, vec![]); // repaint so the control re-enables
    sim.clone_req = false;
    sim.submitted = false;
    sim.click(&ctx, clone_btn.center());
    assert!(sim.clone_req, "Clone click raises the one-shot request");

    assert!(!sim.submitted, "Clone click does not submit");

    // IN FLIGHT: every input returns false, flags are forced clean,
    // and the status line names the repo.
    sim.cloning = Some(("github.com/acme/widget".into(), "widget".into()));
    sim.clone_req = true; // hostile sticky flag: paint must force it low
    sim.browse = true;
    sim.submitted = false;
    let out = sim.frame(&ctx, vec![sw_enter_event()]);
    assert!(!sim.submitted, "busy card returns false on Enter");
    assert!(!sim.clone_req, "busy card forces the clone flag LOW");
    assert!(!sim.browse, "busy card forces the browse flag LOW");
    assert!(
        sw_text_pos(&out, "Cloning widget from GitHub…").is_some(),
        "in-flight status line painted"
    );
    sim.cloning = None;
}

#[test]
fn sw_clone_ticket_five_unparseable_past_surfaces_readable_banner_and_spawns_nothing() {
    // AC guard: invalid pastes show a readable error near the URL
    // field and NEVER spawn a clone worker.
    let mut app = KooladeApp {
        conn_github: "notaurl".into(),
        ..Default::default()
    };
    app.begin_clone_from_field();
    assert!(app.clone_job.is_none(), "no worker spawned for junk");
    let err = app.conn_error.as_deref().unwrap_or("");
    assert!(
        err.starts_with("Can't clone that URL"),
        "readable error surfaced near the field (got: {err})"
    );

    let mut wrong_host = KooladeApp {
        conn_github: "https://gitlab.example.com/acme/site".into(),
        ..Default::default()
    };
    wrong_host.begin_clone_from_field();
    assert!(
        wrong_host.clone_job.is_none(),
        "non-GitHub host spawns nothing"
    );
    assert!(wrong_host.conn_error.is_some(), "host diagnostic surfaced");

    let mut blank = KooladeApp {
        conn_github: "   ".into(),
        ..Default::default()
    };
    blank.begin_clone_from_field();
    assert!(
        blank.clone_job.is_none() && blank.conn_error.is_none(),
        "blank paste is fully inert"
    );
}
