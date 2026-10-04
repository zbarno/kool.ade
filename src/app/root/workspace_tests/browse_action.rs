#[test]
fn sw_welcome_browse_button_signals_request_when_clicked() {
    let mut conn = String::new();
    let mut gh = String::new();
    let mut clone_flag = false;
    let mut req = false;
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0));

    let mut idle = |req_out: &mut bool| {
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                events: vec![],
                ..Default::default()
            },
            |ui| {
                crate::app::welcome::paint(
                    ui,
                    &mut conn,
                    &mut gh,
                    None,
                    req_out,
                    &mut clone_flag,
                    None,
                    None,
                    None,
                );
            },
        );
        out.textures_delta.clear(); // headless: no GPU consumer
        out
    };

    // A fresh context\u{2019}s first pass is placeholders only: burn it,
    // then probe for the Browse button \u{2014} the only ~96x42 rect on the bare
    // welcome surface.
    idle(&mut req);
    assert!(!req, "an idle frame makes no request");
    let out = idle(&mut req);
    let btn = out
        .shapes
        .iter()
        .find_map(|sl| match &sl.shape {
            egui::Shape::Rect(r) => (((r.rect.size().x - 96.0).abs() < 2.01)
                && ((r.rect.size().y - 42.0).abs() < 2.01))
                .then_some(r.rect),
            _ => None,
        })
        .unwrap_or_else(|| panic!("Browse button rect missing from painted shapes"));
    assert!(
        btn.intersects(screen),
        "the button sits inside the viewport"
    );
    let pos = btn.center();

    // Acting frame: press+release on the button in a single frame.
    let mut req2 = false;
    let mut clone2 = false;
    let btn_evt = egui::PointerButton::Primary;
    let mods = Default::default();
    let mut acted = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            events: vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: btn_evt,
                    pressed: true,
                    modifiers: mods,
                },
                egui::Event::PointerButton {
                    pos,
                    button: btn_evt,
                    pressed: false,
                    modifiers: mods,
                },
            ],
            ..Default::default()
        },
        |ui| {
            crate::app::welcome::paint(
                ui,
                &mut conn,
                &mut gh,
                None,
                &mut req2,
                &mut clone2,
                None,
                None,
                None,
            );
        },
    );
    acted.textures_delta.clear(); // headless: no GPU consumer
    assert!(
        req2,
        "clicking Browse\u{2026} raises the one-shot request flag"
    );
}
