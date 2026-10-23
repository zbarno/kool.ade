use std::{path::Path, process::Command};

pub(super) fn checkout(root: &Path) -> String {
    let status = Command::new("git")
        .args([
            "status",
            "--short",
            "--ignored",
            "--untracked-files=all",
            "--",
            "apps/web",
        ])
        .current_dir(root)
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_else(|error| format!("git status unavailable: {error}"));
    let hardlinks = Command::new("find")
        .args([
            "apps/web", "-type", "f", "-links", "+1", "-printf", "%n %p\\n",
        ])
        .current_dir(root)
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_else(|error| format!("hard-link scan unavailable: {error}"));
    let size = Command::new("du")
        .args(["-sh", "apps/web"])
        .current_dir(root)
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_else(|error| format!("workspace size unavailable: {error}"));

    format!(
        "status={}; hardlinks={}; size={}",
        bounded(&status),
        bounded(&hardlinks),
        bounded(&size)
    )
}

fn bounded(value: &str) -> &str {
    let boundary = value.floor_char_boundary(value.len().min(2_000));
    &value[..boundary]
}
