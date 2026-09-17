//! Visual identity: a calm dark slate palette with restrained accents.
//! Everything in the UI draws its colors from here so the look stays coherent
//! (SPECIFICATION.md §22 "clean, professional visual identity").

use egui::{Color32, CornerRadius, Stroke};

pub const BG: Color32 = Color32::from_rgb(32, 32, 32);
pub const PANEL: Color32 = Color32::from_rgb(43, 43, 43);
pub const PANEL_ALT: Color32 = Color32::from_rgb(53, 53, 53);
pub const BORDER: Color32 = Color32::from_rgb(65, 65, 65);
pub const TEXT: Color32 = Color32::from_rgb(236, 234, 230);
pub const TEXT_DIM: Color32 = Color32::from_rgb(157, 157, 153);
pub const ACCENT: Color32 = Color32::from_rgb(218, 223, 212);
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(66, 74, 64);
pub const DANGER: Color32 = Color32::from_rgb(255, 107, 107);
pub const WARNING: Color32 = Color32::from_rgb(255, 180, 84);
pub const SUCCESS: Color32 = Color32::from_rgb(107, 212, 144);
pub const PURPLE: Color32 = Color32::from_rgb(167, 139, 250);
/// Assumption card hue (CHG-002 recorded default, overridable by a fresh
/// word); DANGER red stays deliberately unclaimed as a card hue (stip. ii).
pub const ASSUMPTION_LAVENDER: Color32 = Color32::from_rgb(190, 168, 255);
/// Ownership card hue (CHG-002 recorded default, overridable by a fresh
/// word); DANGER red stays deliberately unclaimed as a card hue (stip. ii).
pub const OWNERSHIP_PINK: Color32 = Color32::from_rgb(232, 145, 190);

/// Centralised class-to-card-hue mapping (CHG-002 recorded defaults):
/// Task-story (`None`) -> ACCENT sage, Question -> PURPLE, Ambiguity ->
/// SUCCESS green, Assumption -> ASSUMPTION_LAVENDER, Ownership ->
/// OWNERSHIP_PINK. Every card still prints its class header, and DANGER red
/// is deliberately unclaimed so the red activity line stays singular on the
/// very task cards it decorates.
pub fn board_hue(kind: Option<crate::domain::ItemKind>) -> Color32 {
    use crate::domain::ItemKind::*;
    match kind {
        None => ACCENT,
        Some(Question) => PURPLE,
        Some(Ambiguity) => SUCCESS,
        Some(Assumption) => ASSUMPTION_LAVENDER,
        Some(Ownership) => OWNERSHIP_PINK,
    }
}

/// Frame for a board card: corner radius 6, inner margin 10, a fill leaned
/// toward the class hue (0.22 gamma blend active, 0.12 idle), and a
/// class-keyed stroke. Active-state rule (CHG-002 stip. i): a running Task
/// card already wears ACCENT as its base hue, so a colour promotion would be
/// a no-op there - it earns the heavier 2.5 px stroke in addition to the fill
/// shift. Every item class keeps the 1.0 px width but upgrades its stroke
/// colour to ACCENT, which contrasts structurally against its base hue. Idle
/// strokes stay on the class hue in every case.
pub fn board_frame(kind: Option<crate::domain::ItemKind>, active: bool) -> egui::Frame {
    let hue = board_hue(kind);
    let (stroke_px, stroke_color) = match (active, kind) {
        (true, None) => (2.5, ACCENT),
        (true, Some(_)) => (1.0, ACCENT),
        (false, _) => (1.0, hue),
    };
    egui::Frame::NONE
        .corner_radius(6)
        .inner_margin(10)
        .fill(PANEL.lerp_to_gamma(hue, if active { 0.22 } else { 0.12 }))
        .stroke(Stroke::new(stroke_px, stroke_color))
}

/// Base style applied once during creation.
pub fn packet_visuals() -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    v.window_fill = BG;
    v.panel_fill = BG;
    v.window_fill = PANEL;
    v.extreme_bg_color = PANEL_ALT;
    v.widgets.noninteractive.bg_fill = PANEL;
    v.widgets.noninteractive.fg_stroke.color = TEXT;
    v.widgets.noninteractive.corner_radius = CornerRadius::same(8);
    v.widgets.inactive.bg_fill = PANEL_ALT;
    v.widgets.inactive.fg_stroke.color = TEXT_DIM;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.hovered.bg_fill = PANEL_ALT;
    v.widgets.hovered.fg_stroke.color = TEXT;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT_SOFT);
    v.widgets.active.bg_fill = ACCENT_SOFT;
    v.widgets.active.fg_stroke.color = TEXT;
    v.selection.bg_fill = ACCENT_SOFT;
    v.hyperlink_color = ACCENT;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.inactive.weak_bg_fill = PANEL_ALT;
    v.widgets.hovered.weak_bg_fill = BORDER;
    v.widgets.inactive.corner_radius = CornerRadius::same(8);
    v.widgets.hovered.corner_radius = CornerRadius::same(8);
    v.widgets.active.corner_radius = CornerRadius::same(8);
    v
}

/// Rounded framed box helper (cards in panes).
pub fn card_frame() -> egui::Frame {
    egui::Frame::NONE
        .fill(PANEL)
        .corner_radius(14.0)
        .stroke(Stroke::new(1.0, BORDER))
        .inner_margin(egui::Margin::same(20))
}

/// Badge chip (priorities, kinds, statuses).
pub fn badge(ui: &mut egui::Ui, text: &str, bg: Color32, fg: Color32) -> egui::Response {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), egui::FontId::proportional(10.5), fg);
    let size = galley.size() + egui::vec2(10.0, 6.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().rect_filled(rect, 4.0, bg);
    ui.painter()
        .galley(rect.left_top() + egui::vec2(5.0, 3.0), galley, fg);
    resp
}

impl crate::domain::Priority {
    /// Chip color pairing for badges.
    pub fn badge_colors(&self) -> (Color32, Color32) {
        match self {
            crate::domain::Priority::Blocking => (Color32::from_rgb(80, 26, 26), DANGER),
            crate::domain::Priority::High => (Color32::from_rgb(80, 56, 20), WARNING),
            crate::domain::Priority::Normal => (ACCENT_SOFT, ACCENT),
        }
    }
}

impl crate::domain::ItemKind {
    pub fn badge_colors(&self) -> (Color32, Color32) {
        match self {
            crate::domain::ItemKind::Question => (PURPLE_DARK, PURPLE),
            crate::domain::ItemKind::Ambiguity => (Color32::from_rgb(24, 66, 56), SUCCESS),
            crate::domain::ItemKind::Assumption => (
                Color32::from_rgb(56, 44, 80),
                ASSUMPTION_LAVENDER,
            ),
            crate::domain::ItemKind::Ownership => (
                Color32::from_rgb(74, 38, 62),
                OWNERSHIP_PINK,
            ),
        }
    }
}

const PURPLE_DARK: Color32 = Color32::from_rgb(50, 40, 84);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ItemKind::*;
    use egui::Color32;

    /// All five board discriminants return the CHG-002 recorded defaults
    /// byte-for-byte (literal pins on purpose, so a future rebasing of the
    /// ACCENT/PURPLE/etc palette constants fails loudly), and each item-kind
    /// badge shares its foreground with the board hue of the same class.
    #[test]
    fn board_class_hues_match_recorded_defaults() {
        let kinds = [
            None,
            Some(Question),
            Some(Ambiguity),
            Some(Assumption),
            Some(Ownership),
        ];
        let recorded_defaults = [
            Color32::from_rgb(218, 223, 212),
            Color32::from_rgb(167, 139, 250),
            Color32::from_rgb(107, 212, 144),
            Color32::from_rgb(190, 168, 255),
            Color32::from_rgb(232, 145, 190),
        ];
        assert_eq!(kinds.len(), recorded_defaults.len());
        for (kind, expected) in kinds.into_iter().zip(recorded_defaults) {
            assert_eq!(
                board_hue(kind),
                expected,
                "unexpected hue for {:?}",
                kind
            );
        }
        assert_eq!(Question.badge_colors().1, board_hue(Some(Question)));
        assert_eq!(Ambiguity.badge_colors().1, board_hue(Some(Ambiguity)));
        assert_eq!(Assumption.badge_colors().1, board_hue(Some(Assumption)));
        assert_eq!(Ownership.badge_colors().1, board_hue(Some(Ownership)));
    }

    /// Stipulation ii, machine-guarded: DANGER red is not a card hue. The
    /// sentinel is pinned first so the guard cannot pass vacuously, and all
    /// five current discriminants - the four ItemKind arms plus the Task
    /// `None` arm - reject red; add any new ItemKind variant to this list
    /// (whatever arm it reaches) to keep the guard exhaustive.
    #[test]
    fn no_card_hues_claim_danger() {
        assert_eq!(DANGER, Color32::from_rgb(255, 107, 107));
        for kind in [
            None,
            Some(Question),
            Some(Ambiguity),
            Some(Assumption),
            Some(Ownership),
        ] {
            assert_ne!(
                board_hue(kind),
                DANGER,
                "board_hue({:?}) must never claim DANGER red",
                kind
            );
        }
    }

    /// Carry-forward of the incumbent's strongest surviving clause: no two
    /// card hues may collide, catching a future single-arm typo that
    /// reintroduces a duplicate even when both colliding values are
    /// individually reasonable.
    #[test]
    fn board_hues_remain_pairwise_distinct() {
        let kinds = [
            None,
            Some(Question),
            Some(Ambiguity),
            Some(Assumption),
            Some(Ownership),
        ];
        let hues = kinds.map(board_hue);
        for (index, kind) in kinds.into_iter().enumerate() {
            assert!(
                !hues[..index].contains(&hues[index]),
                "duplicate card hue for {:?} at index {}",
                kind,
                index
            );
        }
    }

    /// Stipulation i + REQ-F20-2: every running card separates from its idle
    /// twin in fill (0.22 vs 0.12 hue blend) AND in stroke (width or colour).
    /// Specialisation: the running Task card earns the 2.5 px ACCENT stroke
    /// over its 1.0 px idle (its base hue already equals ACCENT, so width is
    /// the distinguishing channel), and every item class keeps 1.0 px while
    /// promoting stroke colour to ACCENT against its idle class-hue stroke.
    #[test]
    fn board_frames_separate_active_from_idle_per_class() {
        let kinds = [
            None,
            Some(Question),
            Some(Ambiguity),
            Some(Assumption),
            Some(Ownership),
        ];
        for kind in kinds {
            let idle = board_frame(kind, false);
            let running = board_frame(kind, true);
            assert_ne!(
                running.fill, idle.fill,
                "fill shift lost for {:?}",
                kind
            );
            assert!(
                running.stroke.width != idle.stroke.width
                    || running.stroke.color != idle.stroke.color,
                "running stroke indistinguishable from idle for {:?}",
                kind
            );
        }
        let idle_task = board_frame(None, false);
        let running_task = board_frame(None, true);
        assert_eq!(running_task.stroke.width, 2.5);
        assert_eq!(idle_task.stroke.width, 1.0);
        assert_eq!(running_task.stroke.color, ACCENT);
        for kind in [
            Some(Question),
            Some(Ambiguity),
            Some(Assumption),
            Some(Ownership),
        ] {
            let idle = board_frame(kind, false);
            let running = board_frame(kind, true);
            assert_eq!(running.stroke.color, ACCENT);
            assert_eq!(running.stroke.width, 1.0);
            assert_eq!(idle.stroke.color, board_hue(kind));
        }
    }

    #[test]
    fn badge_impls_compile() {
        let (a, b) = crate::domain::Priority::High.badge_colors();
        let (c, d) = crate::domain::ItemKind::Question.badge_colors();
        let _ = (a, b, c, d);
    }
}
