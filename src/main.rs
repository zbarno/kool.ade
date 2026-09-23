//! Packet — git-native LLM specification planner (desktop entry point).

fn main() -> eframe::Result {
    packet::diagnostics::install();
    let result = eframe::run_native(
        "Packet — git-native specification planner",
        packet::app::options(),
        Box::new(|_cc: &eframe::CreationContext| {
            Ok(Box::new(packet::app::PacketApp::default()) as Box<dyn eframe::App>)
        }),
    );
    if let Err(error) = &result {
        packet::diagnostics::record_startup_error(&format!("{error:?}"));
    }
    result
}
