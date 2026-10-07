/// Paint the browse card body; returns `(choose_pressed, cancel_pressed)`,
/// the `(save, close)` tuple convention of the house cards. Single click
/// SELECTS; double click (or Enter, which re-confirms the current folder)
/// DESCENDS; the up row ascends. No button here initiates a connect.
use super::*;

pub fn paint_browse_card(ui: &mut egui::Ui, dlg: &mut DlgBrowse) -> (bool, bool) {
    // Cheap liveness probe: if `current` ceased to exist while the modal sat
    // open, re-degrade once to the "cannot read" notice.
    if !dlg.current.is_dir() {
        dlg.refresh_rows();
    }

    ui.label(
        RichText::new("Directories only. Green names sit inside a git working tree.")
            .weak()
            .size(11.0),
    );
    ui.add_space(6.0);

    // Swap the rows out so navigation inside the loop can reborrow `dlg`.
    // Remembers the view so a mid-loop descent/ascend (which re-lists into
    // `dlg.rows`) is not clobbered by restoring the stale snapshot.
    let view_before = dlg.current.clone();
    let rows = std::mem::take(&mut dlg.rows);
    for row in &rows {
        let name = if row.up {
            String::from("..")
        } else {
            row.path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| row.path.display().to_string())
        };
        let label = RichText::new(name).color(if row.git { theme::SUCCESS } else { theme::TEXT });
        let response = ui.selectable_label(dlg.selected == row.path, label);
        if response.double_clicked() {
            if row.up {
                dlg.ascend();
            } else {
                dlg.descend_into(row.path.clone());
            }
        } else if response.clicked() {
            dlg.selected = row.path.clone();
        }
    }
    // If the loop navigated, `dlg.rows` already holds the fresh listing —
    // putting the pre-navigation snapshot back would freeze the view on the
    // old directory. Otherwise restore the snapshot verbatim.
    if dlg.current == view_before {
        dlg.rows = rows;
    }

    if ui.ctx().input(|i| i.key_pressed(egui::Key::Enter)) {
        dlg.descend_into(dlg.current.clone());
    }

    if let Some(error) = &dlg.read_error {
        ui.add_space(4.0);
        ui.label(
            RichText::new(format!("Cannot read this folder: {error}"))
                .weak()
                .size(11.0)
                .color(theme::TEXT_DIM),
        );
    }

    ui.add_space(10.0);
    let full = dlg.selected.to_string_lossy().into_owned();
    ui.add(
        egui::Label::new(
            RichText::new(full.clone())
                .font(egui::FontId::monospace(12.5))
                .color(theme::TEXT_DIM),
        )
        .truncate(),
    )
    .on_hover_text(full);
    ui.add_space(10.0);

    let can_choose = dlg.selected.exists() && dlg.selected.is_dir();
    let mut choose = false;
    let mut cancel = false;
    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
        let choose_btn = ui.add_enabled(
            can_choose,
            egui::Button::new(RichText::new("Choose folder").strong().color(theme::TEXT))
                .fill(theme::ACCENT_SOFT)
                .corner_radius(6.0),
        );
        if choose_btn.clicked() {
            choose = true;
        }
        if ui.button(RichText::new("Cancel").weak()).clicked() {
            cancel = true;
        }
        ui.add_space(4.0);
    });
    (choose, cancel)
}
