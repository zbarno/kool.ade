use super::super::super::*;
use std::path::{Path, PathBuf};
pub(super) fn sw_fixture(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() % 1_000_000_000_000u128)
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "swtest-{}-{}-{:03}-{tag}",
        std::process::id(),
        nanos,
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("fixture dir created");
    dir
}

pub(super) fn sw_git_init(dir: &Path) {
    let st = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["init", "-q"])
        .status()
        .expect("ambient git CLI available for fixture setup");
    assert!(st.success(), "git init must succeed in {dir:?}");
}

pub(super) fn sw_stems(rows: &[DirRow]) -> Vec<String> {
    rows.iter()
        .map(|r| {
            if r.up {
                String::from("..")
            } else {
                r.path
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default()
            }
        })
        .collect()
}

pub(super) fn sw_click(pos: egui::Pos2) -> Vec<egui::Event> {
    let btn = egui::PointerButton::Primary;
    let mods = egui::Modifiers::default();
    vec![
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
    ]
}

pub(super) fn sw_frame(
    w: f32,
    h: f32,
    events: Vec<egui::Event>,
    time: Option<f64>,
) -> egui::RawInput {
    let mut input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(w, h),
        )),
        events,
        ..Default::default()
    };
    if let Some(time) = time {
        input.time = Some(time);
    }
    input
}

/// Paint one browse frame inside the real production modal chrome.
/// Returns (shapes-out, modal_closed, choose_pressed, cancel_pressed).
pub(super) fn sw_run_frame(
    ctx: &egui::Context,
    dlg: &mut DlgBrowse,
    input: egui::RawInput,
) -> (egui::FullOutput, bool, bool, bool) {
    let mut closed = false;
    let mut choose = false;
    let mut cancel = false;
    let mut out = ctx.run_ui(input, |ui| {
        closed =
            crate::ui::overlays::show_modal(ui, true, "Choose a workspace folder", 560.0, |ui| {
                (choose, cancel) = paint_browse_card(ui, dlg);
            });
    });
    out.textures_delta.clear(); // no GPU consumer in-process
    (out, closed, choose, cancel)
}

/// Consume a frame's first pass: a brand-new `egui::Context` emits only
/// `Shape::Noop` placeholders on its very first frame (fonts and pass
/// state still settling), so geometry/text lookups are unreliable there.
/// Every frame-driven test spends one thrown-away idle frame here first —
/// the same discipline the overlays modal tests apply (they locate panel
/// geometry only from their third frame on).
pub(super) fn sw_warm(ctx: &egui::Context, dlg: &mut DlgBrowse) {
    sw_run_frame(ctx, dlg, sw_frame(1280.0, 800.0, Vec::new(), None));
}

/// Centre of a whole-word text shape (row labels, buttons). Colours can
/// be colour-managed at paint time, so text — never fill — anchors hits.
pub(super) fn sw_text_pos(out: &egui::FullOutput, needle: &str) -> Option<egui::Pos2> {
    out.shapes.iter().find_map(|sl| match &sl.shape {
        egui::Shape::Text(t) if t.galley.text() == needle => {
            Some(t.pos + t.galley.mesh_bounds.center().to_vec2())
        }
        _ => None,
    })
}

/// Rect of the accent-filled “Choose folder” button — the only rounded-six
/// wide rect painted inside this modal (colour-independent predicate).
pub(super) fn sw_choose_rect(out: &egui::FullOutput) -> egui::Rect {
    let mut acc = Vec::new();
    for shape_like in out.shapes.iter() {
        let egui::Shape::Rect(r) = &shape_like.shape else {
            continue;
        };
        if (r.corner_radius.nw as f32 - 6.0).abs() < 0.51 && r.rect.size().x > 60.0 {
            acc.push(r.rect);
        }
    }
    assert!(
        !acc.is_empty(),
        "Choose-folder button rect missing from painted shapes"
    );
    acc[0]
}

pub(super) fn sw_name_of(row: &DirRow) -> String {
    row.path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}
