use std::cell::RefCell;

use crate::app::bootstrap_remediation::TrustedProgram;

use super::harness::{ResponsePlan, execute, runner};

#[test]
fn staged_transaction_checks_keys_then_installs_the_same_paths_offline() {
    let runner = runner(ResponsePlan::default());
    let repository_checks = RefCell::new(Vec::new());
    let result = execute(&runner, || {
        repository_checks
            .borrow_mut()
            .push(runner.calls.borrow().len());
        Ok(())
    })
    .unwrap();

    assert_eq!(result.exit_code, 0);
    assert_eq!(*repository_checks.borrow(), [2, 3, 4, 5, 14]);
    let calls = runner.calls.borrow();
    assert_eq!(
        calls.iter().map(|call| call.0).collect::<Vec<_>>(),
        [
            TrustedProgram::Dd,
            TrustedProgram::Dd,
            TrustedProgram::Dnf,
            TrustedProgram::Dnf,
            TrustedProgram::Mkdir,
            TrustedProgram::Dnf,
            TrustedProgram::Find,
            TrustedProgram::Sha256sum,
            TrustedProgram::RpmDb,
            TrustedProgram::RpmKeys,
            TrustedProgram::RpmKeys,
            TrustedProgram::RpmKeys,
            TrustedProgram::Rpm,
            TrustedProgram::Rpm,
            TrustedProgram::Find,
            TrustedProgram::Sha256sum,
            TrustedProgram::RpmKeys,
            TrustedProgram::Dnf,
            TrustedProgram::Rm,
        ]
    );

    let download = &calls[5].1;
    let install_at = download
        .iter()
        .position(|value| value == "install")
        .unwrap();
    let download_only_at = download
        .iter()
        .position(|value| value == "--downloadonly")
        .unwrap();
    let no_downgrade_at = download
        .iter()
        .position(|value| value == "--no-allow-downgrade")
        .unwrap();
    assert_eq!(install_at + 1, download_only_at);
    assert_eq!(download_only_at + 1, no_downgrade_at);
    assert!(
        download
            .iter()
            .any(|value| { value.starts_with("--setopt=destdir=/run/koolade-rootless-dnf-") })
    );
    for option in [
        "--setopt=pkg_gpgcheck=0",
        "--setopt=fedora.pkg_gpgcheck=0",
        "--setopt=updates.pkg_gpgcheck=0",
        "--setopt=koolade-docker-ce-stable.pkg_gpgcheck=0",
    ] {
        assert!(
            download.iter().any(|value| value == option),
            "missing {option}"
        );
    }

    let signature_check = &calls[11].1;
    let offline_install = &calls[17].1;
    let checked_path = signature_check.last().unwrap();
    assert!(checked_path.ends_with("/packages/docker-ce-1.0-1.x86_64.rpm"));
    assert!(calls[4].1.iter().any(|value| value.ends_with("/cache")));
    assert!(
        signature_check
            .iter()
            .any(|value| value == "_pkgverify_level all")
    );
    assert!(
        signature_check
            .iter()
            .any(|value| value == "_pkgverify_flags 0x0")
    );
    assert!(offline_install.contains(checked_path));
    assert!(offline_install.iter().any(|value| {
        value.starts_with("--setopt=cachedir=/run/koolade-rootless-dnf-")
            && value.ends_with("/cache")
    }));
    assert!(offline_install.iter().any(|value| {
        value.starts_with("--setopt=system_cachedir=/run/koolade-rootless-dnf-")
            && value.ends_with("/cache")
    }));
    assert!(offline_install.contains(&"--disablerepo=*".to_owned()));
    assert!(offline_install.contains(&"--cacheonly".to_owned()));
    let offline_install_at = offline_install
        .iter()
        .position(|value| value == "install")
        .unwrap();
    let offline_no_downgrade_at = offline_install
        .iter()
        .position(|value| value == "--no-allow-downgrade")
        .unwrap();
    assert_eq!(offline_install_at + 1, offline_no_downgrade_at);
    assert!(offline_install.contains(&"--setopt=localpkg_gpgcheck=0".to_owned()));
    assert!(offline_install.contains(&"--setopt=installonlypkgs=docker-ce".to_owned()));
    assert!(offline_install.contains(&"--setopt=installonly_limit=0".to_owned()));
    assert!(offline_install.contains(&"--setopt=obsoletes=False".to_owned()));
    assert!(
        !offline_install
            .iter()
            .any(|value| value.starts_with("--enablerepo="))
    );
    assert!(!offline_install.contains(&"--downloadonly".to_owned()));
    assert!(!offline_install.contains(&"--allowerasing".to_owned()));
    assert!(!offline_install.contains(&"--skip-broken".to_owned()));
    assert_eq!(calls[18].0, TrustedProgram::Rm);
}
