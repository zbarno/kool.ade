use std::{fs, path::Path};

pub(super) fn write_app(root: &Path) {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runtime_qa/falsely_healthy_web");
    for relative in ["build.py", "src/server.py", "src/index.html", "test_app.py"] {
        let source = fixture.join(relative);
        let destination = root.join(relative);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::copy(source, destination).unwrap();
    }
}
