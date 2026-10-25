use std::{fs, path::Path};

pub(super) fn write_go_headless_app(root: &Path) {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runtime_qa/go_headless");
    for relative in [
        "go.mod",
        "go.sum",
        "cmd/api/main.go",
        "cmd/api/main_test.go",
        "dist/.gitkeep",
    ] {
        let source = fixture.join(relative);
        let destination = root.join(relative);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::copy(source, destination).unwrap();
    }
}
