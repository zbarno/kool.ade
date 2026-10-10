use std::process::{Command, Stdio};

pub(super) fn put(text: &str) {
    for program in ["pbcopy", "wl-copy", "xclip"] {
        if let Ok(mut child) = Command::new(program)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            use std::io::Write;
            if let Some(stdin) = child.stdin.as_mut() {
                let _ = stdin.write_all(text.as_bytes());
            }
            if child.wait().is_ok_and(|status| status.success()) {
                return;
            }
        }
    }
}
