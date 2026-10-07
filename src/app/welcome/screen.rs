use crate::ui::theme;
use egui::{Frame, RichText, TextEdit};

mod presentation;
/// Paint the centered card on the connect screen.
///
/// Raises `*browse_requested` when the 'Browse…' button beside the path
/// field is clicked, and `*clone_requested` when the 'Clone' button is
/// clicked OR Enter is pressed while the GitHub URL field owns focus. The
/// former global Enter hook is retired: it is now focus-scoped, so a bare
/// window-level Enter with no field focused is inert (the Open button
/// covers the mouse path; both fields keep click-to-focus + Tab
/// navigation). While `cloning` is `Some` the card paints the status line,
/// repaints every frame, and swallows ALL input (mirrors story 2's modal
/// discipline). Returning `true` means the existing Open/Enter submit
/// should run.
/// Secondary-button shell for the connect card (Browse… / Clone).
/// With `disabled` true, renders the sunk look — darkest palette fill,
/// muted border, faded text — because egui 0.36 dropped `enabled`/
/// `gray_out` on concrete widgets; callers ALSO gate their `clicked()`
/// checks on `!busy` so the sink is genuinely inert.
pub(super) fn button_shell(label: &str, disabled: bool) -> egui::Button<'_> {
    if disabled {
        egui::Button::new(RichText::new(label).size(13.0).color(theme::TEXT_DIM))
            .fill(theme::BG)
            .stroke(egui::Stroke::new(1.0, theme::BORDER))
    } else {
        egui::Button::new(RichText::new(label).size(13.0))
    }
}

#[allow(clippy::too_many_arguments)] // card state bag plus optional script-test probes
pub fn paint(
    card_ui: &mut egui::Ui,
    path: &mut String,
    github_url: &mut String,
    error: Option<&str>,
    browse_requested: &mut bool,
    clone_requested: &mut bool,
    cloning: Option<(&str, &str)>,
    path_field_probe: Option<&mut (egui::Id, egui::Rect)>,
    url_field_probe: Option<&mut (egui::Id, egui::Rect)>,
) -> bool {
    let busy = cloning.is_some();
    // Snapshot focus BEFORE any field is laid out: single-line TextEdits
    // SURRENDER focus the moment they observe Enter (handled deep inside
    // their own update), so a late query would always read "none" on the
    // very Enter frame that must scope the keystroke. State entering this
    // frame == what the operator perceives as focus at keytime.
    let pre_focus: Option<egui::Id> = card_ui.memory(|m| m.focused());
    card_ui.set_width(card_ui.available_width().min(620.0));
    card_ui.with_layout(egui::Layout::top_down(egui::Align::Min), |card_ui| {
        presentation::hero(card_ui);
        card_ui.add_space(20.0);
        card_ui.label(
            RichText::new("Make something great.")
                .size(if card_ui.available_width() < 380.0 { 26.0 } else { 34.0 })
                .strong()
                .color(theme::TEXT),
        );
        card_ui.label(
            RichText::new("Open a project. Shape the plan. Get to work.")
                .size(13.0)
                .color(theme::TEXT_DIM),
        );
        card_ui.add_space(8.0);
        presentation::section_label(card_ui, "PROJECT FOLDER");
        let path_field = card_ui
            .horizontal(|ui| {
                let field_width = (ui.available_width() - 96.0 - ui.spacing().item_spacing.x).max(80.0);
                // While the clone runs the row is VISUALLY DISABLED: a dimmed
                // mono read-out stands in for the live editor (truly inert).
                let field = if busy {
                    ui.add_sized(
                        egui::vec2(field_width, 42.0),
                        egui::Label::new(
                            RichText::new(if path.trim().is_empty() {
                                "/path/to/my/project".to_string()
                            } else {
                                path.clone()
                            })
                            .family(egui::FontFamily::Monospace)
                            .size(12.5)
                            .color(theme::TEXT_DIM),
                        ),
                    )
                } else {
                    ui.add_sized(
                        egui::vec2(field_width, 42.0),
                        TextEdit::singleline(path)
                            .hint_text("/path/to/my/project")
                            .margin(egui::vec2(12.0, 12.0))
                            .desired_width(f32::INFINITY)
                            .font(egui::FontId::monospace(12.5)),
                    )
                };
                let browse = ui.add_sized(egui::vec2(96.0, 42.0), button_shell("Browse…", busy));
                if !busy && browse.clicked() {
                    *browse_requested = true;
                }
                field
            })
            .inner;
        if let Some(p) = path_field_probe {
            p.0 = path_field.id;
            p.1 = path_field.rect;
        }
        super::repository_picker::paint(card_ui, path);
        card_ui.add_space(6.0);
        let submit_btn = egui::Button::new(
            RichText::new("Open workspace")
                .strong()
                .size(15.0)
                .color(if busy { theme::TEXT_DIM } else { theme::TEXT }),
        )
        .fill(if busy {
            theme::ACCENT_SOFT
        } else {
            theme::ACTION
        })
        .corner_radius(6.0);
        let submit = card_ui.add_sized(egui::vec2(card_ui.available_width(), 50.0), submit_btn);
        if !busy && submit.hovered() {
            card_ui.ctx().request_repaint();
        }
        card_ui.add_space(12.0);
        card_ui.separator();
        card_ui.add_space(8.0);
        presentation::section_label(card_ui, "OR CLONE FROM GITHUB");
        let url_field = card_ui
            .horizontal(|ui| {
                let field_width =
                    (ui.available_width() - 110.0 - ui.spacing().item_spacing.x).max(80.0);
                let field = if busy {
                    ui.add_sized(
                        egui::vec2(field_width, 42.0),
                        egui::Label::new(
                            RichText::new(if github_url.trim().is_empty() {
                                "https://github.com/{owner}/{repo}".to_string()
                            } else {
                                github_url.clone()
                            })
                            .family(egui::FontFamily::Monospace)
                            .size(12.5)
                            .color(theme::TEXT_DIM),
                        ),
                    )
                } else {
                    ui.add_sized(
                        egui::vec2(field_width, 42.0),
                        TextEdit::singleline(github_url)
                            .hint_text("https://github.com/{owner}/{repo}")
                            .margin(egui::vec2(12.0, 12.0))
                            .desired_width(f32::INFINITY)
                            .font(egui::FontId::monospace(12.5)),
                    )
                };
                let clone = ui.add_sized(
                    egui::vec2(110.0, 42.0),
                    // Disabled while a clone runs AND whenever the trimmed URL
                    // is empty (ticket: the control mirrors the path field's
                    // empty-silence; begin_clone double-guards anyway).
                    button_shell("Clone", busy || github_url.trim().is_empty()),
                );
                if !busy && !github_url.trim().is_empty() && clone.clicked() {
                    *clone_requested = true;
                }
                field
            })
            .inner;
        if let Some(p) = url_field_probe {
            p.0 = url_field.id;
            p.1 = url_field.rect;
        }
        if let Some((_label, repo)) = cloning {
            card_ui.add_space(6.0);
            card_ui.label(
                RichText::new(format!("Cloning {repo} from GitHub…"))
                    .size(12.5)
                    .color(theme::TEXT_DIM),
            );
            card_ui.ctx().request_repaint();
            // Input-stealing while the worker runs: swallow every signal and
            // hand the caller back a clean, actionable-nothing frame.
            *clone_requested = false;
            *browse_requested = false;
            return false;
        }
        if let Some(e) = error {
            Frame::NONE
                .fill(egui::Color32::from_rgb(58, 24, 24))
                .corner_radius(6.0)
                .inner_margin(egui::Margin::symmetric(10, 8))
                .show(card_ui, |ui| {
                    ui.label(RichText::new(e).color(theme::DANGER).size(12.5));
                });
            card_ui.add_space(6.0);
        }
        card_ui.add_space(4.0);
        card_ui.label(
            RichText::new(
                "Plan, answer questions, and approve work from your board.\nYour specifications and decisions stay with your project.",
            )
            .weak()
            .size(11.0),
        );
        // Focus-scoped Enter (the retired global hook's replacement): the PATH
        // field's Enter submits (with the incumbent empty guard); the URL
        // field's Enter requests a clone; any other Enter does nothing.
        let entered =
            card_ui.input(|i| i.key_pressed(egui::Key::Enter)) && card_ui.input(|i| !i.modifiers.ctrl);
        if entered {
            if Some(path_field.id) == pre_focus {
                if !path.trim().is_empty() {
                    return true;
                }
            } else if Some(url_field.id) == pre_focus {
                *clone_requested = true;
            }
        }
        submit.clicked()
    }).inner
}
