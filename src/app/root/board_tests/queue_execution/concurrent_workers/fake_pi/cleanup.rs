use std::{path::Path, time::Duration};

pub(super) fn remove_test_root(root: &Path) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        match std::fs::remove_dir_all(root) {
            Ok(()) => return,
            Err(error)
                if error.kind() == std::io::ErrorKind::DirectoryNotEmpty
                    && std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(error) => panic!("remove queue fixture root {}: {error}", root.display()),
        }
    }
}
