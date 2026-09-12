//! Desktop session wiring (eframe). Entry point: [`root::PacketApp`].

pub mod dialogs;
pub mod root;
pub mod session;
pub mod welcome;

pub use root::PacketApp;

/// Native-window bootstrap options (initial size floor for the three-pane layout).
pub fn options() -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1480.0, 900.0])
            .with_min_inner_size([1080.0, 640.0]),
        ..Default::default()
    }
}

pub mod manager;
