use super::*;

// ---- CHG-003 welcome screen ↔ workspace-browser dialog -----------------
//
// House practice (F-16): the app pump needs an eframe::Frame (GPU
// object), so tests drive the two production pieces directly —
// `welcome::paint` for the button and `KooladeApp::render_dialog` for
// the dialog contract — exactly how the other dialogs are exercised in
// this codebase.

/// Route one frame of the PARKED dialog through the production router.
pub(super) fn sw_route(
    ctx: &egui::Context,
    app: &mut KooladeApp,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let mut parked = Some(
        app.dialog
            .take()
            .expect("a dialog is parked for this route"),
    );
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        ctx.style_mut_of(theme, |style| style.animation_time = 0.0);
    }
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1400.0, 900.0),
            )),
            events,
            ..Default::default()
        },
        |ui| app.render_dialog(ui, parked.take().expect("parked dialog for this route")),
    );
    // No GPU consumer in-process: drain textures before the output dies.
    out.textures_delta.clear();
    out
}

/// Locate a whole-word text shape; centre position of its mesh.
pub(super) fn sw_text_pos(output: &egui::FullOutput, needle: &str) -> Option<egui::Pos2> {
    output.shapes.iter().find_map(|shape| {
        let egui::Shape::Text(text) = &shape.shape else {
            return None;
        };
        (text.galley.text() == needle)
            .then(|| text.pos + text.galley.mesh_bounds.center().to_vec2())
    })
}

/// Consume a fresh context's first pass, which paints placeholder (Noop)
/// shapes only. Call once per `egui::Context` before trusting geometry —
/// the same warm-up discipline the overlays modal tests apply. Leaves the
/// parked dialog parked (idle frame).
pub(super) fn sw_warm_route(ctx: &egui::Context, app: &mut KooladeApp) {
    let _out = sw_route(ctx, app, Vec::new());
}

/// One-frame click (move -> press -> release) at an absolute position.
pub(super) fn sw_route_click_at(ctx: &egui::Context, app: &mut KooladeApp, pos: egui::Pos2) {
    let btn = egui::PointerButton::Primary;
    let mods = Default::default();
    let _ = sw_route(
        ctx,
        app,
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: btn,
                pressed: true,
                modifiers: mods,
            },
            egui::Event::PointerButton {
                pos,
                button: btn,
                pressed: false,
                modifiers: mods,
            },
        ],
    );
}

/// Locate `label` by its painted text, then click it in one frame. The
/// locating frame assumes the context has already been warmed.
pub(super) fn sw_route_click_by_label(ctx: &egui::Context, app: &mut KooladeApp, label: &str) {
    let out = sw_route(ctx, app, Vec::new());
    let pos = sw_text_pos(&out, label)
        .unwrap_or_else(|| panic!("no \u{2018}{label}\u{2019} painted in the dialog frame"));
    sw_route_click_at(ctx, app, pos);
}

/// Centre of the modal\u{2019}s top-right close box (28x28, 17px in from the
/// panel edges: 16 padding + 1 stroke).
pub(super) fn sw_close_pos(panel: egui::Rect) -> egui::Pos2 {
    egui::Pos2::new(panel.max.x - 31.0, panel.min.y + 31.0)
}

/// The \u{201c}Choose folder\u{201d} button: the modal\u{2019}s rounded-6 rect wider than a
/// fist (colour-independent predicate; the panel frame rounds at 12).
pub(super) fn sw_choose_rect(out: &egui::FullOutput) -> egui::Rect {
    out.shapes
        .iter()
        .find_map(|sl| match &sl.shape {
            egui::Shape::Rect(r) => ((r.corner_radius.nw as f32 - 6.0).abs() < 0.51
                && r.rect.size().x > 60.0)
                .then_some(r.rect),
            _ => None,
        })
        .unwrap_or_else(|| panic!("Choose-folder button rect missing from painted shapes"))
}

// =================================================================
// CHG-003: GitHub URL clone (wiring + card behaviour)
// =================================================================

/// Controllable stand-in for the clone worker: `compute` runs only
/// after the gate receives. Deterministic settle-between-ticks;
/// the join handle lets the ticker observe real settlement state.
pub(super) fn gated_worker<F>(
    compute: F,
) -> (
    std::thread::JoinHandle<Result<std::path::PathBuf, crate::error::AppError>>,
    std::sync::mpsc::Sender<()>,
)
where
    F: FnOnce() -> Result<std::path::PathBuf, crate::error::AppError> + Send + 'static,
{
    let (gate_tx, gate_rx) = std::sync::mpsc::channel::<()>();
    let join = std::thread::spawn(move || {
        let _ = gate_rx.recv();
        compute()
    });
    (join, gate_tx)
}

/// Poll (≤5s) until the ticker sees its job's worker as settled.
pub(super) fn await_settle(app: &KooladeApp) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while app
        .clone_job
        .as_ref()
        .is_some_and(|j| !j.join.is_finished())
    {
        assert!(std::time::Instant::now() < deadline, "worker never settled");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

/// mkrepo-style source shaped like a VANILLA GitHub repository: init
/// -b main, LOCAL identity, one committed baseline and DELIBERATELY
/// no planning/ — the connect pipeline must bootstrap + migrate it
/// exactly as it does for a hand-typed path into the same tree.
pub(super) fn swcl_repo(tag: &str) -> std::path::PathBuf {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let repo = std::env::temp_dir().join(format!("swcl_{tag}_{seq}_{}", std::process::id()));
    std::fs::create_dir_all(&repo).unwrap();
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(args)
            .status()
            .unwrap();
        assert!(
            status.success(),
            "git {args:?} failed building the {tag} repo"
        );
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.name", "SW Clone Test"]);
    git(&["config", "user.email", "sw-clone@example.invalid"]);
    git(&["config", "commit.gpgsign", "false"]);
    std::fs::write(repo.join("README.md"), "# Cloned by SW test\n").unwrap();
    git(&["add", "README.md"]);
    git(&["commit", "-q", "-m", "baseline"]);
    repo
}

pub(super) fn sw_cl_job(
    join: std::thread::JoinHandle<Result<std::path::PathBuf, crate::error::AppError>>,
) -> CloneJob {
    CloneJob {
        url_display: "github.com/acme/site".into(),
        repo: "site".into(),
        join,
    }
}

/// One paint-frame driver for the connect card, standing in for the
/// Welcome arm (fresh per-frame flag cells approximated by resets in
/// the test bodies).
pub(super) struct SwCardSim {
    pub(super) path: String,
    pub(super) github: String,
    pub(super) err: Option<String>,
    pub(super) browse: bool,
    pub(super) clone_req: bool,
    pub(super) submitted: bool,
    pub(super) cloning: Option<(String, String)>,
    /// Field hit-geometry captured inside `paint` every frame — the
    /// reliable ground truth for scripted clicks (painted fills are
    /// theme-dependent; hit geometry is not).
    pub(super) path_probe: (egui::Id, egui::Rect),
    pub(super) url_probe: (egui::Id, egui::Rect),
}

impl SwCardSim {
    pub(super) fn frame(
        &mut self,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1400.0, 900.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                self.submitted = welcome::paint(
                    ui,
                    &mut self.path,
                    &mut self.github,
                    self.err.as_deref(),
                    &mut self.browse,
                    &mut self.clone_req,
                    self.cloning.as_ref().map(|(u, r)| (u.as_str(), r.as_str())),
                    Some(&mut self.path_probe),
                    Some(&mut self.url_probe),
                );
            },
        );
        out.textures_delta.clear();
        out
    }

    pub(super) fn click(&mut self, ctx: &egui::Context, pos: egui::Pos2) {
        let btn = egui::PointerButton::Primary;
        let mods = Default::default();
        self.frame(
            ctx,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: btn,
                    pressed: true,
                    modifiers: mods,
                },
                egui::Event::PointerButton {
                    pos,
                    button: btn,
                    pressed: false,
                    modifiers: mods,
                },
            ],
        );
    }
}

pub(super) fn sw_enter_event() -> egui::Event {
    egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        modifiers: Default::default(),
        pressed: true,
        repeat: false,
    }
}

/// The two field hit-rects straight from the in-paint probe (path
/// first, URL second) — immune to theme-dependent painted fills.
pub(super) fn sw_card_fields(sim: &SwCardSim) -> (egui::Rect, egui::Rect) {
    let (path_id, path_rect) = &sim.path_probe;
    let (url_id, url_rect) = &sim.url_probe;
    assert_ne!(*path_id, egui::Id::NULL, "path field was not painted");
    assert_ne!(*url_id, egui::Id::NULL, "url field was not painted");
    for (r, what) in [(path_rect, "path"), (url_rect, "url")] {
        assert!(
            r.height() > 30.0 && r.width() > 100.0,
            "{what} field rect looks degenerate: {r:?}"
        );
    }
    (*path_rect, *url_rect)
}
