use super::{QemuProcessError, qemu_img_version_is_valid, runner::run_bounded};
use std::{
    ffi::OsStr, fs, os::unix::fs::PermissionsExt, path::Path, process::Command, time::Duration,
};

#[test]
fn qemu_img_version_probe_requires_strict_bounded_utf8_version_output() {
    // Production boundary: readiness must not treat lossy/malformed version
    // output as a valid QEMU image utility. Direct parser test; no VM or tools.
    assert!(qemu_img_version_is_valid(
        b"qemu-img version 9.2.0 (Debian 1:9.2+ds-1)\n"
    ));
    assert!(qemu_img_version_is_valid(b"qemu-img version 2.0.0\n"));
    for output in [
        b"qemu-img version nightly\n".as_slice(),
        b"qemu-img version 9.2.0 \xff\n".as_slice(),
        b"other tool version 9.2.0\n".as_slice(),
        b"qemu-img version 9.2.0\x1b\n".as_slice(),
    ] {
        assert!(!qemu_img_version_is_valid(output));
    }
}

#[test]
fn timeout_kills_the_child_process_group_and_bounds_captured_output() {
    // Production boundary: QEMU must not hang the launch thread or grow host
    // memory from tool output. This exercises process-group handling with a
    // local fixture; no VM, Docker, network, or SSH; low setup cost.
    let temp = tempfile::tempdir().unwrap();
    let fake = temp.path().join("fake-qemu-img");
    write_executable(
        &fake,
        "#!/bin/sh\n(sleep 20) &\nprintf '%070000d' 1\nwait\n",
    );
    assert!(matches!(
        run_bounded(
            &fake,
            &[OsStr::new("--version")],
            Duration::from_millis(100)
        ),
        Err(QemuProcessError::TimedOut)
    ));
}

#[test]
fn exited_leader_cannot_leave_pipe_reader_blocked_by_a_descendant() {
    // Production boundary: a tool leader may exit while a same-group helper
    // retains stdout/stderr. The deadline still kills that group and returns
    // without joining a blocked reader; local shell only, low setup cost.
    let temp = tempfile::tempdir().unwrap();
    let fake = temp.path().join("fake-qemu-img");
    write_executable(&fake, "#!/bin/sh\n(sleep 20) &\nexit 0\n");
    let started = std::time::Instant::now();
    assert!(matches!(
        run_bounded(
            &fake,
            &[OsStr::new("--version")],
            Duration::from_millis(150)
        ),
        Err(QemuProcessError::TimedOut)
    ));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn leader_exit_kills_helpers_that_closed_both_pipes() {
    // Production boundary: helper cleanup is required even when the leader
    // closes output before exiting. Marker absence proves the helper did not
    // outlive a successful invocation; local shell only; low setup cost.
    let temp = tempfile::tempdir().unwrap();
    let fake = temp.path().join("fake-qemu-img");
    write_executable(
        &fake,
        "#!/bin/sh\n(sleep 0.4; echo alive > \"$2\") >/dev/null 2>&1 &\nexit 0\n",
    );
    let marker = temp.path().join("helper-survived");
    let output = run_bounded(
        &fake,
        &[OsStr::new("--version"), marker.as_os_str()],
        Duration::from_secs(2),
    )
    .unwrap();
    assert!(output.status.success());
    std::thread::sleep(Duration::from_millis(500));
    assert!(!marker.exists());
}

#[cfg(target_os = "linux")]
#[test]
fn qemu_image_helper_cannot_escape_its_process_group_with_setsid() {
    // Production boundary: QEMU image utility descendants cannot escape timeout
    // cleanup via setsid. Requires /bin/sh, util-linux setsid, and sleep; no VM,
    // Docker, network, or SSH; temporary marker and low setup cost.
    let temp = tempfile::tempdir().unwrap();
    let fake = temp.path().join("fake-qemu-img");
    write_executable(
        &fake,
        "#!/bin/sh\n/usr/bin/setsid /bin/sh -c 'sleep 0.3; touch \"$1\"' child \"$2\" >/dev/null 2>&1 &\nexit 0\n",
    );
    let marker = temp.path().join("escaped-helper");
    let output = run_bounded(
        &fake,
        &[OsStr::new("--version"), marker.as_os_str()],
        Duration::from_secs(2),
    )
    .unwrap();
    assert!(output.status.success());
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        !marker.exists(),
        "setsid helper escaped QEMU tool confinement"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn qemu_image_process_runs_with_no_new_privileges() {
    // Production boundary: QEMU image tools cannot gain privilege through
    // setuid bits or file capabilities at exec. A shell reads kernel status;
    // no VM, Docker, network, or SSH; temporary setup is negligible.
    let temp = tempfile::tempdir().unwrap();
    let fake = temp.path().join("fake-qemu-img");
    write_executable(&fake, "#!/bin/sh\ngrep '^NoNewPrivs:' /proc/self/status\n");
    let output = run_bounded(&fake, &[OsStr::new("--version")], Duration::from_secs(2)).unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"NoNewPrivs:\t1\n");
}

#[test]
fn runner_preserves_non_utf8_argument_bytes() {
    // Production boundary: image paths must reach QEMU without lossy Unicode
    // conversion. A local shell fixture writes argv bytes verbatim; no VM,
    // Docker, network, or SSH; low setup cost.
    use std::os::unix::ffi::OsStrExt;

    let temp = tempfile::tempdir().unwrap();
    let fake = temp.path().join("fake-qemu-img");
    write_executable(&fake, "#!/bin/sh\nprintf '%s' \"$1\" > \"$2\"\n");
    let argument = OsStr::from_bytes(b"image-\xff.qcow2");
    let captured = temp.path().join("captured-argument");
    let output = run_bounded(
        &fake,
        &[argument, captured.as_os_str()],
        Duration::from_secs(2),
    )
    .unwrap();
    assert!(output.status.success());
    assert_eq!(fs::read(captured).unwrap(), b"image-\xff.qcow2");
}

#[test]
fn output_capture_rejects_overflow_without_growing_memory() {
    // Production boundary: successful helper output is capped before parsing.
    // The shell writes a deterministic oversized line; no VM/network; low setup.
    let temp = tempfile::tempdir().unwrap();
    let fake = temp.path().join("fake-qemu-img");
    write_executable(&fake, "#!/bin/sh\nprintf '%070000d' 1\n");
    assert!(matches!(
        run_bounded(&fake, &[OsStr::new("--version")], Duration::from_secs(2)),
        Err(QemuProcessError::OutputTooLarge)
    ));
}

#[test]
fn builder_cleanup_uses_privileged_removal_only_for_its_mktemp_directory() {
    // Production boundary: debootstrap creates root-owned build files, so EXIT
    // cleanup must invoke privileged removal for its generated private temp root
    // and preserve siblings. Bootstrap is stopped early; no real package/mount,
    // VM, network, or SSH; low setup cost.
    let temp = tempfile::tempdir().unwrap();
    let fake_bin = temp.path().join("bin");
    let runner_temp = temp.path().join("runner-temp");
    fs::create_dir(&fake_bin).unwrap();
    fs::create_dir(&runner_temp).unwrap();
    let sibling = runner_temp.join("unrelated-data");
    fs::write(&sibling, b"keep").unwrap();
    let real_path = std::env::var("PATH").unwrap();
    for name in [
        "qemu-img",
        "mkfs.ext4",
        "mount",
        "umount",
        "grub-install",
        "chroot",
        "sha256sum",
        "parted",
        "losetup",
    ] {
        write_executable(&fake_bin.join(name), "#!/bin/sh\nexit 0\n");
    }
    write_executable(&fake_bin.join("sudo"), "#!/bin/sh\nexec \"$@\"\n");
    let agent_binary = temp.path().join("agent");
    write_executable(&agent_binary, "#!/bin/sh\nexit 0\n");
    write_executable(
        &fake_bin.join("debootstrap"),
        "#!/bin/sh\nfor arg do case \"$arg\" in */rootfs) mkdir -p \"$arg\"; echo root-owned > \"$arg/root-owned-file\";; esac; done\nexit 17\n",
    );
    let script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("containers/computer-use/build-image.sh");
    let result = Command::new("bash")
        .arg(script)
        .arg(temp.path().join("output"))
        .arg(&agent_binary)
        .env("PATH", format!("{}:{real_path}", fake_bin.display()))
        .env("RUNNER_TEMP", &runner_temp)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(17));
    assert!(String::from_utf8_lossy(&result.stderr).contains("failed after 3 attempts"));
    assert_eq!(fs::read(&sibling).unwrap(), b"keep");
    assert_eq!(fs::read_dir(&runner_temp).unwrap().count(), 1);
}

#[test]
fn debootstrap_download_adapter_limits_urls_and_retries_tls_downloads() {
    // Production boundary: debootstrap package requests pass through this adapter;
    // only pinned Debian snapshot URLs may be fetched, with TLS verification and
    // bounded retries. A fake curl records argv; no network or privileged setup.
    let temp = tempfile::tempdir().unwrap();
    let fake_bin = temp.path().join("bin");
    fs::create_dir(&fake_bin).unwrap();
    let argument_log = temp.path().join("curl-arguments");
    write_executable(
        &fake_bin.join("curl"),
        "#!/bin/bash\nprintf '%s\\n' \"$@\" >> \"$CURL_ARGUMENT_LOG\"\noutput=\nurl=\nwhile (($#)); do case \"$1\" in --output) output=$2; shift 2;; https://*) url=$1; shift;; *) shift;; esac; done\nif [[ \"$url\" == https://snapshot.debian.org/* ]]; then exit 22; fi\n[[ \"$url\" == https://deb.debian.org/debian/pool/* ]] || exit 2\n: > \"$output\"\n",
    );
    let adapter =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("containers/computer-use/debootstrap-wget");
    let output_path = temp.path().join("package.deb");
    let real_path = std::env::var("PATH").unwrap();
    let result = Command::new(&adapter)
        .args([
            "-q",
            "-O",
            output_path.to_str().unwrap(),
            "https://snapshot.debian.org/archive/debian/20261101T000000Z/pool/main/package.deb",
        ])
        .env("PATH", format!("{}:{real_path}", fake_bin.display()))
        .env("CURL_ARGUMENT_LOG", &argument_log)
        .output()
        .unwrap();
    assert!(result.status.success());
    let arguments = fs::read_to_string(argument_log).unwrap();
    assert!(arguments.lines().any(|arg| arg == "--retry-all-errors"));
    assert!(arguments.lines().any(|arg| arg == "--retry"));
    assert!(arguments.lines().any(|arg| arg == "--output"));
    assert!(arguments.lines().any(|arg| arg
        == "https://snapshot.debian.org/archive/debian/20261101T000000Z/pool/main/package.deb"));
    assert!(
        arguments
            .lines()
            .any(|arg| arg == "https://deb.debian.org/debian/pool/main/package.deb")
    );
    assert!(
        arguments
            .lines()
            .any(|arg| arg == output_path.to_string_lossy())
    );

    let rejected = Command::new(&adapter)
        .args([
            "-O",
            output_path.to_str().unwrap(),
            "https://example.com/package.deb",
        ])
        .output()
        .unwrap();
    assert_eq!(rejected.status.code(), Some(2));
}

fn write_executable(path: &Path, content: &str) {
    fs::write(path, content).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}
