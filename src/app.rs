//! Desktop session wiring (eframe). Entry point: [`root::PacketApp`].

pub mod dialogs;
pub mod root;
pub mod session;
pub(crate) mod setup_attention;
pub mod spawn;
pub mod welcome;

pub use root::{PacketApp, options};

#[cfg(test)]
mod tests {
    #[test]
    fn native_window_can_enter_the_compact_layout() {
        let viewport = super::options().viewport;
        let minimum = viewport.min_inner_size.expect("minimum window size");

        assert!(minimum.x < 960.0);
        assert!(minimum.x <= 360.0);
        assert!(minimum.y <= 480.0);
    }
}

pub mod manager;
pub mod reconciliation_lifecycle;
