//! Shared viewport sizing and spacing for dialogs and workspaces.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceKind {
    Small,
    Medium,
    Workspace,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceBounds {
    pub rect: egui::Rect,
    pub header_height: f32,
    pub footer_height: f32,
}

impl SurfaceBounds {
    pub fn for_viewport(viewport: egui::Rect, kind: SurfaceKind, requested_width: f32) -> Self {
        let margin = match kind {
            SurfaceKind::Small => 24.0,
            SurfaceKind::Medium => 24.0,
            SurfaceKind::Workspace => 20.0,
        };
        let available_width = (viewport.width() - margin * 2.0).max(0.0);
        let (preferred, max_ratio, max_width, height_ratio, header, footer) = match kind {
            SurfaceKind::Small => (requested_width.min(560.0), 1.0, 560.0, 0.78, 48.0, 0.0),
            SurfaceKind::Medium => (requested_width.max(720.0), 0.94, 900.0, 0.90, 52.0, 62.0),
            SurfaceKind::Workspace => (available_width * 0.93, 0.95, 1680.0, 0.92, 58.0, 0.0),
        };
        let width = preferred
            .min(available_width * max_ratio)
            .min(max_width)
            .max(0.0);
        let height = (viewport.height() * height_ratio)
            .min((viewport.height() - margin * 2.0).max(0.0))
            .max(0.0);
        Self {
            rect: egui::Rect::from_center_size(viewport.center(), egui::vec2(width, height)),
            header_height: header,
            footer_height: footer,
        }
    }

    pub fn content_height(self) -> f32 {
        (self.rect.height() - self.header_height - self.footer_height).max(80.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surfaces_scale_with_viewport_and_stay_inside_it() {
        for size in [
            egui::vec2(320.0, 240.0),
            egui::vec2(1280.0, 720.0),
            egui::vec2(2560.0, 1440.0),
        ] {
            let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            let workspace = SurfaceBounds::for_viewport(viewport, SurfaceKind::Workspace, 0.0);
            let medium = SurfaceBounds::for_viewport(viewport, SurfaceKind::Medium, 800.0);
            assert!(viewport.contains_rect(workspace.rect));
            assert!(viewport.contains_rect(medium.rect));
            assert!(workspace.rect.width() <= 1680.0);
            assert!(medium.rect.width() <= 900.0);
        }
    }
}
