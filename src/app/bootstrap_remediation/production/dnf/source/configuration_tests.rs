use super::inspect_configuration_dropin_contents;

#[test]
fn candidate_repository_locator_and_key_dropins_are_rejected() {
    for contents in [
        b"[fedora*]\nbaseurl=https://mirror.invalid/fedora/\n".as_slice(),
        b"[updates]\ngpgkey=file:///tmp/untrusted-fedora-key\n".as_slice(),
        b"[koolade-*]\nmetalink=https://mirror.invalid/docker/\n".as_slice(),
        b"[fedora]\npkg_gpgcheck=0\n".as_slice(),
    ] {
        assert!(inspect_configuration_dropin_contents(contents).is_err());
    }
}

#[test]
fn global_origin_overrides_in_config_dropins_are_rejected() {
    for contents in [
        b"[main]\nbaseurl=https://mirror.invalid/fedora/\n".as_slice(),
        b"[main]\ngpgkey=file:///tmp/untrusted-fedora-key\n".as_slice(),
        b"[main]\nsslverify=0\n".as_slice(),
        b"[main]\nsslcacert=/tmp/untrusted-ca.pem\n".as_slice(),
        b"[main]\nskip_if_unavailable=true\n".as_slice(),
        b"[main]\npkg_gpgcheck=0\n".as_slice(),
    ] {
        assert!(inspect_configuration_dropin_contents(contents).is_err());
    }
}

#[test]
fn global_cache_redirects_in_config_dropins_are_rejected() {
    for contents in [
        b"[main]\ncachedir=/tmp/dnf-cache\n".as_slice(),
        b"[main]\nsystem_cachedir=/tmp/dnf-cache\n".as_slice(),
    ] {
        assert!(inspect_configuration_dropin_contents(contents).is_err());
    }
    inspect_configuration_dropin_contents(
        b"[main]\ncachedir=/var/cache/libdnf5\nsystem_cachedir=/var/cache/libdnf5\npkg_gpgcheck=1\n",
    )
    .unwrap();
}
