//! Packet — git-native LLM specification planner (desktop entry point).

fn main() -> eframe::Result {
    eframe::run_native(
        "Packet — git-native specification planner",
        packet::app::options(),
        Box::new(|_cc: &eframe::CreationContext| {
            Ok(Box::new(packet::app::PacketApp::default()) as Box<dyn eframe::App>)
        }),
    )
}
