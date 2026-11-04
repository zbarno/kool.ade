use std::path::{Path, PathBuf};

pub(super) fn fake_gh(root: &Path) -> PathBuf {
    let gh = root.join("gh");
    std::fs::write(
        &gh,
        "#!/bin/sh\n[ \"$1\" = api ] && [ \"$2\" = user ] || exit 2\nprintf '987654321\\n'\n",
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o700)).unwrap();
    gh
}
