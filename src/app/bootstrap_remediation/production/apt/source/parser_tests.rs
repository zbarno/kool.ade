use super::inspect_source_contents;
use crate::app::setup_attention::catalog::LinuxRootlessRecipeV1 as Recipe;

const DOCKER: &str = "Types: deb\nURIs: https://download.docker.com/linux/ubuntu\nSuites: noble\nComponents: stable\nArchitectures: amd64\nSigned-By: /etc/apt/keyrings/docker.asc\n";

#[test]
fn accepts_only_recipe_stock_sources_and_the_pinned_docker_source() {
    let cases = [
        (
            Recipe::Debian13Amd64,
            "Types: deb deb-src\nURIs: https://deb.debian.org/debian\nSuites: trixie trixie-updates\nComponents: main contrib non-free non-free-firmware\nSigned-By: /usr/share/keyrings/debian-archive-keyring.gpg\n\nTypes: deb deb-src\nURIs: https://security.debian.org/debian-security\nSuites: trixie-security\nComponents: main contrib non-free non-free-firmware\nSigned-By: /usr/share/keyrings/debian-archive-keyring.gpg\n",
        ),
        (
            Recipe::Ubuntu2404Amd64,
            "Types: deb\nURIs: http://archive.ubuntu.com/ubuntu\nSuites: noble noble-updates noble-backports\nComponents: main restricted universe multiverse\nSigned-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg\n\nTypes: deb\nURIs: http://security.ubuntu.com/ubuntu\nSuites: noble-security\nComponents: main restricted universe multiverse\nSigned-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg\n",
        ),
        (
            Recipe::PopOs2404Amd64,
            "X-Repolib-Name: Pop_OS System Sources\nEnabled: yes\nTypes: deb deb-src\nURIs: http://apt.pop-os.org/ubuntu\nSuites: noble noble-security noble-updates noble-backports\nComponents: main restricted universe multiverse\nSigned-By: /etc/apt/trusted.gpg.d/ubuntu-keyring-2018-archive.gpg\nX-Repolib-ID: system\nX-Repolib-Default-Mirror: http://apt.pop-os.org/ubuntu\n\nTypes: deb deb-src\nURIs: http://apt.pop-os.org/release\nSuites: noble\nComponents: main\nSigned-By: /etc/apt/trusted.gpg.d/pop-keyring-2017-archive.gpg\n\nTypes: deb\nURIs: http://apt.pop-os.org/proprietary\nSuites: noble\nComponents: main\nSigned-By: /etc/apt/trusted.gpg.d/pop-keyring-2017-archive.gpg\n",
        ),
    ];
    for (recipe, sources) in cases {
        let inspection = inspect_source_contents(sources.as_bytes(), true, recipe).unwrap();
        assert!(inspection.has_active_source);
        assert!(!inspection.has_unapproved_source, "{recipe:?}");
        assert_eq!(inspection.exact_entries, 0);
    }

    let combined = format!(
        "Types: deb\nURIs: http://archive.ubuntu.com/ubuntu\nSuites: noble noble-updates noble-backports\nComponents: main restricted universe multiverse\nSigned-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg\n\nTypes: deb\nURIs: http://security.ubuntu.com/ubuntu\nSuites: noble-security\nComponents: main restricted universe multiverse\nSigned-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg\n\n{DOCKER}"
    );
    let inspection =
        inspect_source_contents(combined.as_bytes(), true, Recipe::Ubuntu2404Amd64).unwrap();
    assert!(!inspection.has_unapproved_source);
    assert_eq!(inspection.exact_entries, 1);
}

#[test]
fn rejects_active_third_party_sources_and_wrong_stock_keyrings() {
    let cases = [
        (
            Recipe::Debian13Amd64,
            "deb [signed-by=/usr/share/keyrings/debian-archive-keyring.gpg] https://ppa.example.org/debian trixie main\n",
            false,
        ),
        (
            Recipe::Ubuntu2404Amd64,
            "deb [signed-by=/tmp/attacker.gpg] http://archive.ubuntu.com/ubuntu noble main\n",
            false,
        ),
        (
            Recipe::PopOs2404Amd64,
            "Types: deb\nURIs: http://apt.pop-os.org/ubuntu\nSuites: noble\nComponents: main\nSigned-By: /etc/apt/trusted.gpg.d/pop-keyring-2017-archive.gpg\n",
            true,
        ),
    ];
    for (recipe, source, deb822) in cases {
        let inspection = inspect_source_contents(source.as_bytes(), deb822, recipe).unwrap();
        assert!(inspection.has_unapproved_source, "{recipe:?}: {source}");
    }
}

#[test]
fn accepts_legacy_stock_sources_only_with_their_distro_keyring() {
    for (recipe, source) in [
        (
            Recipe::Debian13Amd64,
            "deb [arch=amd64 signed-by=/usr/share/keyrings/debian-archive-keyring.gpg] https://deb.debian.org/debian trixie main contrib non-free non-free-firmware\n",
        ),
        (
            Recipe::Ubuntu2404Amd64,
            "deb [arch=amd64 signed-by=/usr/share/keyrings/ubuntu-archive-keyring.gpg] https://archive.ubuntu.com/ubuntu noble main restricted universe multiverse\n",
        ),
        (
            Recipe::PopOs2404Amd64,
            "deb [arch=amd64 signed-by=/etc/apt/trusted.gpg.d/ubuntu-keyring-2018-archive.gpg] http://apt.pop-os.org/ubuntu noble main restricted universe multiverse\n",
        ),
    ] {
        let inspection = inspect_source_contents(source.as_bytes(), false, recipe).unwrap();
        assert!(inspection.has_active_source);
        assert!(!inspection.has_unapproved_source, "{recipe:?}");
    }
}

#[test]
fn active_sources_reject_security_override_fields_and_options() {
    for source in [
        "deb [signed-by=/usr/share/keyrings/ubuntu-archive-keyring.gpg check-valid-until=no] http://archive.ubuntu.com/ubuntu noble main\n",
        "deb [signed-by=/usr/share/keyrings/ubuntu-archive-keyring.gpg trusted=yes] http://archive.ubuntu.com/ubuntu noble main\n",
        "Types: deb\nURIs: http://archive.ubuntu.com/ubuntu\nSuites: noble\nComponents: main\nSigned-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg\nCheck-Valid-Until: no\n",
        "Types: deb\nURIs: http://archive.ubuntu.com/ubuntu\nSuites: noble\nComponents: main\nSigned-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg\nTrusted: yes\n",
    ] {
        let deb822 = source.starts_with("Types:");
        assert!(
            inspect_source_contents(source.as_bytes(), deb822, Recipe::Ubuntu2404Amd64).is_err(),
            "accepted security override: {source}"
        );
    }
}

#[test]
fn disabled_deb822_third_party_source_is_inactive() {
    let disabled = "Types: deb\nURIs: https://ppa.example.org/ubuntu\nSuites: noble\nComponents: main\nSigned-By: /tmp/ppa.gpg\nEnabled: no\n";
    let inspection =
        inspect_source_contents(disabled.as_bytes(), true, Recipe::Ubuntu2404Amd64).unwrap();
    assert!(!inspection.has_active_source);
    assert!(!inspection.has_unapproved_source);
}
