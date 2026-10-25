use std::{fs, path::Path};

pub(super) fn write_rust_desktop_app(root: &Path) {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/runtime_qa/rust_desktop_acceptance");
    for relative in [
        "Cargo.toml",
        "Cargo.lock",
        "src/main.rs",
        "src/state.rs",
        "src/x11.rs",
        "src/x11/api.rs",
    ] {
        let source = fixture.join(relative);
        let destination = root.join(relative);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::copy(source, destination).unwrap();
    }
}
