//! Kool.ad/e desktop entry point.

fn main() -> eframe::Result {
    koolade::diagnostics::install();
    let result = eframe::run_native(
        "Kool.ad/e",
        koolade::app::options(),
        Box::new(|_cc: &eframe::CreationContext| {
            Ok(Box::new(koolade::app::KooladeApp::default()) as Box<dyn eframe::App>)
        }),
    );
    if let Err(error) = &result {
        koolade::diagnostics::record_startup_error(&format!("{error:?}"));
    }
    result
}
