//! Lightweight, non-modal notifications sliding in from the top-right
//! (confirmations, warnings, errors). Self-expiring; click to dismiss.

use std::time::{Duration, Instant};

use egui::{Align2, Color32, Layout, Order, RichText};

use crate::ui::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Info,
    Success,
    Warning,
    Danger,
}

#[derive(Clone, Debug)]
struct Toast {
    text: String,
    tone: Tone,
    born: Instant,
}

impl Toast {
    fn new(text: String, tone: Tone) -> Self {
        Self {
            text,
            tone,
            born: Instant::now(),
        }
    }

    fn expired(&self) -> bool {
        let ttl = match self.tone {
            Tone::Info | Tone::Success => Duration::from_secs(5),
            Tone::Warning => Duration::from_secs(8),
            Tone::Danger => Duration::from_secs(12),
        };
        self.born.elapsed() > ttl
    }

    fn icon(&self) -> char {
        match self.tone {
            Tone::Info => '\u{2139}',
            Tone::Success => '\u{2714}',
            Tone::Warning => '\u{26A0}',
            Tone::Danger => '\u{2716}',
        }
    }

    fn border(&self) -> Color32 {
        match self.tone {
            Tone::Info => theme::ACCENT,
            Tone::Success => theme::SUCCESS,
            Tone::Warning => theme::WARNING,
            Tone::Danger => theme::DANGER,
        }
    }
}

/// One queue, owned by the app; painted every frame by the UI.
#[derive(Default)]
pub struct ToastQueue {
    items: Vec<Toast>,
}

impl ToastQueue {
    pub fn info(&mut self, text: impl Into<String>) {
        self.items.push(Toast::new(text.into(), Tone::Info));
    }
    pub fn success(&mut self, text: impl Into<String>) {
        self.items.push(Toast::new(text.into(), Tone::Success));
    }
    pub fn warning(&mut self, text: impl Into<String>) {
        self.items.push(Toast::new(text.into(), Tone::Warning));
    }
    pub fn danger(&mut self, text: impl Into<String>) {
        self.items.push(Toast::new(text.into(), Tone::Danger));
    }

    /// Paint the stack (top-right) and expire/dismiss entries.
    pub fn show(&mut self, ctx: &egui::Context) {
        if self.items.is_empty() {
            return;
        }
        let snapshot: Vec<Toast> = self.items.to_vec();
        let mut dismissed_idx: Vec<usize> = Vec::new();
        egui::Area::new("packet_toasts".into())
            .order(Order::Foreground)
            .anchor(Align2::RIGHT_TOP, [12.0, 44.0])
            .layout(Layout::top_down(egui::Align::RIGHT))
            .show(ctx, |ui| {
                ui.set_min_width(280.0);
                for (idx, t) in snapshot.iter().enumerate() {
                    let frame = egui::Frame::NONE
                        .fill(theme::PANEL_ALT)
                        .corner_radius(8.0)
                        .stroke(egui::Stroke::new(1.5, t.border()))
                        .inner_margin(egui::Margin::symmetric(12, 8));
                    let close_btn = frame.show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 7.0;
                            ui.label(RichText::new(t.icon().to_string()).strong());
                            ui.label(RichText::new(&t.text).color(theme::TEXT));
                        });
                        ui.add_space(2.0);
                        crate::ui::overlays::close_button(ui)
                    });
                    if close_btn.response.clicked() {
                        dismissed_idx.push(idx);
                    }
                    ui.add_space(8.0);
                }
            });
        for idx in dismissed_idx.into_iter().rev() {
            if idx < self.items.len() {
                self.items.remove(idx);
            }
        }
        self.items.retain(|t| !t.expired());
        ctx.request_repaint_after(Duration::from_millis(250));
    }
}
