use std::{collections::HashSet, path::Path};

use super::{DOCKER_REPO_PATH, repositories::inspect_repo_contents};
use crate::app::bootstrap_remediation::trusted_assets::DOCKER_DNF_REPO;

const FEDORA_AND_UPDATES_REPOSITORIES: &str = "[fedora]\n#baseurl=http://download.example/pub/fedora/linux/releases/$releasever/Everything/$basearch/os/\nmetalink=https://mirrors.fedoraproject.org/metalink?repo=fedora-$releasever&arch=$basearch\ngpgkey=file:///usr/share/pki/rpm-gpg/RPM-GPG-KEY-fedora-$releasever-$basearch\ngpgcheck=1\n\n[updates]\n#baseurl=http://download.example/pub/fedora/linux/updates/$releasever/Everything/$basearch/\nmetalink=https://mirrors.fedoraproject.org/metalink?repo=updates-released-f$releasever&arch=$basearch\ngpgkey=file:///usr/share/pki/rpm-gpg/RPM-GPG-KEY-fedora-$releasever-$basearch\ngpgcheck=1\n";

fn inspect(path: &Path, contents: &[u8], ids: &mut HashSet<String>) -> anyhow::Result<()> {
    let mut has_fedora = false;
    let mut has_updates = false;
    inspect_repo_contents(path, contents, ids, &mut has_fedora, &mut has_updates)
}

#[test]
fn managed_docker_repository_must_match_exact_pinned_contents() {
    let mut ids = HashSet::new();
    inspect(Path::new(DOCKER_REPO_PATH), DOCKER_DNF_REPO, &mut ids).unwrap();
    let docker_repo = String::from_utf8(DOCKER_DNF_REPO.to_vec()).unwrap();
    let disabled_package_check =
        docker_repo.replace("gpgcheck=1\n", "gpgcheck=1\npkg_gpgcheck=0\n");
    let error = inspect(
        Path::new(DOCKER_REPO_PATH),
        disabled_package_check.as_bytes(),
        &mut HashSet::new(),
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("disables RPM package signature checks")
    );
    assert!(
        inspect(
            Path::new(DOCKER_REPO_PATH),
            b"[koolade-docker-ce-stable]\nenabled=0\n",
            &mut HashSet::new(),
        )
        .is_err()
    );
}

#[test]
fn conflicting_docker_origins_and_repo_ids_are_refused() {
    let path = Path::new("/etc/yum.repos.d/custom.repo");
    assert!(
        inspect(
            path,
            b"[other-docker]\nbaseurl=https://download.docker.com/linux/fedora/45/x86_64/stable\n",
            &mut HashSet::new(),
        )
        .is_err()
    );
    assert!(
        inspect(
            path,
            b"[koolade-docker-ce-stable]\nbaseurl=https://example.invalid/repo\n",
            &mut HashSet::new(),
        )
        .is_err()
    );
}

#[test]
fn fedora_system_repositories_may_not_disable_rpm_signature_checks() {
    assert!(inspect(
        Path::new("/etc/yum.repos.d/fedora.repo"),
        b"[fedora]\nmetalink=https://mirrors.fedoraproject.org/metalink?repo=fedora-$releasever&arch=$basearch\ngpgkey=file:///usr/share/pki/rpm-gpg/RPM-GPG-KEY-fedora-$releasever-$basearch\ngpgcheck=0\n",
        &mut HashSet::new(),
    )
    .is_err());
}

#[test]
fn fedora45_repositories_require_exact_official_metalinks_and_pinned_key() {
    let mut ids = HashSet::new();
    let mut has_fedora = false;
    let mut has_updates = false;
    inspect_repo_contents(
        Path::new("/usr/share/dnf5/repos.d/fedora.repo"),
        FEDORA_AND_UPDATES_REPOSITORIES.as_bytes(),
        &mut ids,
        &mut has_fedora,
        &mut has_updates,
    )
    .unwrap();
    assert!(has_fedora);
    assert!(has_updates);
}

#[test]
fn fedora45_repositories_allow_explicit_secure_transport_settings() {
    let contents = FEDORA_AND_UPDATES_REPOSITORIES.replace(
        "gpgcheck=1",
        "gpgcheck=1\nsslverify=yes\nskip_if_unavailable=false",
    );
    let mut has_fedora = false;
    let mut has_updates = false;
    inspect_repo_contents(
        Path::new("/usr/share/dnf5/repos.d/fedora.repo"),
        contents.as_bytes(),
        &mut HashSet::new(),
        &mut has_fedora,
        &mut has_updates,
    )
    .unwrap();
    assert!(has_fedora && has_updates);
}

#[test]
fn dnf_main_configuration_may_not_redirect_package_metadata_caches() {
    for cache_option in [
        "cachedir=/tmp/dnf-cache",
        "system_cachedir=/tmp/dnf-cache",
        "pkg_gpgcheck=0",
    ] {
        let contents = format!("[main]\n{cache_option}\n{FEDORA_AND_UPDATES_REPOSITORIES}");
        assert!(
            inspect(
                Path::new("/etc/dnf/dnf.conf"),
                contents.as_bytes(),
                &mut HashSet::new(),
            )
            .is_err()
        );
    }
    let contents = format!(
        "[main]\ncachedir=/var/cache/libdnf5\nsystem_cachedir=/var/cache/libdnf5\npkg_gpgcheck=1\n{FEDORA_AND_UPDATES_REPOSITORIES}"
    );
    inspect(
        Path::new("/etc/dnf/dnf.conf"),
        contents.as_bytes(),
        &mut HashSet::new(),
    )
    .unwrap();
}

#[test]
fn fedora_and_updates_repositories_may_not_disable_pkg_gpgcheck() {
    let fedora =
        FEDORA_AND_UPDATES_REPOSITORIES.replacen("gpgcheck=1\n", "gpgcheck=1\npkg_gpgcheck=0\n", 1);
    assert!(
        inspect(
            Path::new("/usr/share/dnf5/repos.d/fedora.repo"),
            fedora.as_bytes(),
            &mut HashSet::new(),
        )
        .is_err()
    );

    let updates = b"[updates]\nmetalink=https://mirrors.fedoraproject.org/metalink?repo=updates-released-f$releasever&arch=$basearch\ngpgkey=file:///usr/share/pki/rpm-gpg/RPM-GPG-KEY-fedora-$releasever-$basearch\ngpgcheck=1\npkg_gpgcheck=0\n";
    assert!(
        inspect(
            Path::new("/usr/share/dnf5/repos.d/updates.repo"),
            updates,
            &mut HashSet::new(),
        )
        .is_err()
    );
}

#[test]
fn fedora45_repositories_reject_competing_or_unpinned_active_settings() {
    for invalid in [
        FEDORA_AND_UPDATES_REPOSITORIES.replace(
            "metalink=https://mirrors.fedoraproject.org/metalink?repo=fedora-$releasever&arch=$basearch\n",
            "metalink=https://mirrors.fedoraproject.org/metalink?repo=fedora-$releasever&arch=$basearch\nbaseurl=https://mirror.invalid/fedora/45/\n",
        ),
        FEDORA_AND_UPDATES_REPOSITORIES.replace(
            "metalink=https://mirrors.fedoraproject.org/metalink?repo=updates-released-f$releasever&arch=$basearch\n",
            "metalink=https://mirrors.fedoraproject.org/metalink?repo=updates-released-f$releasever&arch=$basearch\nmirrorlist=https://mirror.invalid/updates\n",
        ),
        FEDORA_AND_UPDATES_REPOSITORIES.replace(
            "metalink=https://mirrors.fedoraproject.org/metalink?repo=fedora-$releasever&arch=$basearch\n",
            "metalink=https://mirrors.fedoraproject.org/metalink?repo=fedora-$releasever&arch=$basearch\nmetalink=https://mirrors.fedoraproject.org/metalink?repo=fedora-$releasever&arch=$basearch\n",
        ),
        FEDORA_AND_UPDATES_REPOSITORIES.replace(
            "metalink=https://mirrors.fedoraproject.org/metalink?repo=fedora-$releasever&arch=$basearch",
            "metalink=https://mirror.invalid/fedora",
        ),
        FEDORA_AND_UPDATES_REPOSITORIES.replace(
            "gpgkey=file:///usr/share/pki/rpm-gpg/RPM-GPG-KEY-fedora-$releasever-$basearch\n",
            "gpgkey=file:///usr/share/pki/rpm-gpg/RPM-GPG-KEY-fedora-$releasever-$basearch\ngpgkey=https://mirror.invalid/key\n",
        ),
        FEDORA_AND_UPDATES_REPOSITORIES.replace(
            "file:///usr/share/pki/rpm-gpg/RPM-GPG-KEY-fedora-$releasever-$basearch",
            "file:///etc/pki/rpm-gpg/untrusted.key",
        ),
        FEDORA_AND_UPDATES_REPOSITORIES.replace("gpgcheck=1", "gpgcheck=0"),
        FEDORA_AND_UPDATES_REPOSITORIES.replacen("gpgcheck=1\n", "", 1),
        format!("[main]\ngpgkey=https://mirror.invalid/key\n{FEDORA_AND_UPDATES_REPOSITORIES}"),
        format!("[main]\nsslverify=0\n{FEDORA_AND_UPDATES_REPOSITORIES}"),
        format!("[main]\nsslcacert=/tmp/untrusted-ca.pem\n{FEDORA_AND_UPDATES_REPOSITORIES}"),
        format!("[main]\nskip_if_unavailable=1\n{FEDORA_AND_UPDATES_REPOSITORIES}"),
        FEDORA_AND_UPDATES_REPOSITORIES.replace(
            "gpgcheck=1",
            "gpgcheck=1\nsslverify=0",
        ),
        FEDORA_AND_UPDATES_REPOSITORIES.replace(
            "gpgcheck=1",
            "gpgcheck=1\nskip_if_unavailable=true",
        ),
        FEDORA_AND_UPDATES_REPOSITORIES.replace(
            "gpgcheck=1",
            "gpgcheck=1\nproxy_sslverify=false\nproxy_sslcacert=/tmp/proxy-ca.pem",
        ),
        format!("{FEDORA_AND_UPDATES_REPOSITORIES}\n[fedora]\ngpgcheck=1\n"),
    ] {
        let mut has_fedora = false;
        let mut has_updates = false;
        assert!(
            inspect_repo_contents(
                Path::new("/usr/share/dnf5/repos.d/fedora.repo"),
                invalid.as_bytes(),
                &mut HashSet::new(),
                &mut has_fedora,
                &mut has_updates,
            )
            .is_err(),
            "{invalid}"
        );
    }
}

#[test]
fn duplicate_fedora_repo_ids_are_refused() {
    let mut ids = HashSet::new();
    inspect(
        Path::new("/etc/yum.repos.d/fedora.repo"),
        b"[fedora]\nmetalink=https://mirrors.fedoraproject.org/metalink?repo=fedora-$releasever&arch=$basearch\ngpgkey=file:///usr/share/pki/rpm-gpg/RPM-GPG-KEY-fedora-$releasever-$basearch\ngpgcheck=1\n",
        &mut ids,
    )
    .unwrap();
    assert!(
        inspect(
            Path::new("/usr/share/dnf5/repos.d/custom.repo"),
            b"[fedora]\nmetalink=https://mirrors.fedoraproject.org/metalink?repo=fedora-$releasever&arch=$basearch\ngpgkey=file:///usr/share/pki/rpm-gpg/RPM-GPG-KEY-fedora-$releasever-$basearch\ngpgcheck=1\n",
            &mut ids,
        )
        .is_err()
    );
}
