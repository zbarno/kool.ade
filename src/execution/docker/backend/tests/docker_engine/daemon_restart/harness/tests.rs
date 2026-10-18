use std::{os::unix::process::CommandExt, process::Command, time::Duration};

use super::child_exited_without_reaping;

#[test]
fn observes_exit_without_releasing_the_process_group_id() {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg("exit 0")
        .process_group(0)
        .spawn()
        .expect("spawn isolated process-group leader");
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        if child_exited_without_reaping(child.id()) {
            let status = child
                .try_wait()
                .expect("inspect exited child")
                .expect("observed child remains waitable");
            assert!(status.success());
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let _ = child.kill();
    let _ = child.wait();
    panic!("child did not exit before the deadline");
}
