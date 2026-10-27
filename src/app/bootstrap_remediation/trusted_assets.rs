pub(crate) const DOCKER_APT_KEY_SHA256: &str =
    "1500c1f56fa9e26b9b8f42452a553675796ade0807cdce11975eb98170b3a570";
pub(crate) const DOCKER_APT_SOURCES_SHA256: &str =
    "8f33259a79a8149bed86c66e103fb4c3fa70f9219cd7ff315b6cc30988afef0c";
pub(crate) const DOCKER_DNF_KEY_SHA256: &str =
    "e6c650e0700b1bf4868b693b30761b926844befc8a0acb7ac0dd9b1faf1b7423";
pub(crate) const DOCKER_DNF_REPO_SHA256: &str =
    "a42ae70a0e5324097d6b8b3bee465c90daec4d4edc980d1f566919dddc48f96e";
pub(crate) const DOCKER_DNF_KEY_FINGERPRINT: &str = "060A61C51B558A7F742B77AAC52FEB6B621E9F35";
pub(crate) const FEDORA_DNF_KEY_FINGERPRINT: &str = "4F50A6114CD5C6976A7F1179655A4B02F577861E";
pub(crate) const DOCKER_APT_KEY: &[u8] =
    include_bytes!("../../../containers/keys/docker-ce-archive.asc");
pub(crate) const DOCKER_APT_SOURCES: &[u8] = b"Types: deb\nURIs: https://download.docker.com/linux/ubuntu\nSuites: noble\nComponents: stable\nArchitectures: amd64\nSigned-By: /etc/apt/keyrings/docker.asc\n";
pub(crate) const DOCKER_DNF_KEY: &[u8] =
    include_bytes!("../../../containers/keys/docker-fedora.asc");
pub(crate) const DOCKER_DNF_REPO: &[u8] = b"[koolade-docker-ce-stable]\nname=Docker CE Stable - x86_64\nbaseurl=https://download.docker.com/linux/fedora/45/x86_64/stable\nenabled=1\ngpgcheck=1\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-koolade-docker\n";
