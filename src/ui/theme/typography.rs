//! Bundled heading face keeps task typography consistent across native hosts.
use egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily};
use egui::{FontData, FontFamily, FontId};

pub(super) fn install(ctx: &egui::Context) {
    ctx.add_font(FontInsert::new(
        "task-heading",
        FontData::from_static(include_bytes!(
            "../../../assets/fonts/FiraSans-SemiBold.otf"
        )),
        vec![InsertFontFamily {
            family: FontFamily::Name("task-heading".into()),
            priority: FontPriority::Highest,
        }],
    ));
}

pub fn task_heading_font(ctx: &egui::Context, size: f32) -> FontId {
    install(ctx);
    let family = FontFamily::Name("task-heading".into());
    if ctx.fonts(|fonts| fonts.definitions().families.contains_key(&family)) {
        FontId::new(size, family)
    } else {
        ctx.request_repaint();
        FontId::proportional(size)
    }
}
