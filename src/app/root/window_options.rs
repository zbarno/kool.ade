/// Native window options for [`eframe::run_native`]. The minimum stays below
/// the compact-layout breakpoint so the stacked workspace is reachable on
/// smaller displays.
pub fn options() -> eframe::NativeOptions {
    let vp = egui::ViewportBuilder::default()
        .with_inner_size([1480.0, 900.0])
        .with_min_inner_size([360.0, 480.0])
        .with_icon(super::application_icon());
    eframe::NativeOptions {
        viewport: vp,
        ..Default::default()
    }
}
