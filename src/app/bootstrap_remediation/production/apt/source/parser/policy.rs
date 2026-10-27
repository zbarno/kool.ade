use std::collections::BTreeMap;

use self::origins::{docker_repository, expected_keyring, is_distro_source};
use crate::app::setup_attention::catalog::LinuxRootlessRecipeV1;

mod origins;

const DOCKER_KEYRING: &str = "/etc/apt/keyrings/docker.asc";

pub(super) struct EntryInspection {
    pub(super) has_docker_uri: bool,
    pub(super) active: bool,
    pub(super) approved: bool,
    pub(super) exact_docker: bool,
}

pub(super) fn inspect_list_entry(
    line: &str,
    recipe: LinuxRootlessRecipeV1,
) -> anyhow::Result<EntryInspection> {
    let (source_type, rest) = line
        .split_once(char::is_whitespace)
        .ok_or_else(|| anyhow::anyhow!("malformed APT source entry"))?;
    anyhow::ensure!(
        matches!(source_type, "deb" | "deb-src"),
        "unsupported APT source entry"
    );
    let mut rest = rest.trim_start();
    let mut options = BTreeMap::new();
    if let Some(option_text) = rest.strip_prefix('[') {
        let (option_text, source_text) = option_text
            .split_once(']')
            .ok_or_else(|| anyhow::anyhow!("unterminated APT source options"))?;
        for option in option_text.split_whitespace() {
            let (key, value) = option
                .split_once('=')
                .ok_or_else(|| anyhow::anyhow!("unsupported APT source option"))?;
            let key = key.to_ascii_lowercase();
            anyhow::ensure!(
                !is_signature_override(&key),
                "APT source overrides package signature verification"
            );
            anyhow::ensure!(
                matches!(key.as_str(), "arch" | "signed-by")
                    && options.insert(key, value.to_owned()).is_none(),
                "unsupported or duplicate APT source option"
            );
        }
        rest = source_text.trim_start();
    }
    let fields = rest.split_whitespace().collect::<Vec<_>>();
    anyhow::ensure!(fields.len() >= 3, "incomplete APT source entry");
    let uri = fields[0];
    let suite = fields[1];
    let components = &fields[2..];
    let docker = uri.contains("download.docker.com");
    let expected_key = expected_keyring(uri, recipe);
    let key_is_pinned = options
        .get("signed-by")
        .is_some_and(|key| expected_key == Some(key.as_str()));
    let architecture_is_pinned = options
        .get("arch")
        .is_none_or(|architecture| architecture == "amd64");
    let exact_docker = docker
        && source_type == "deb"
        && is_exact_docker_source(uri, suite, components, &options, recipe);
    let approved = if docker {
        exact_docker
    } else {
        key_is_pinned && architecture_is_pinned && is_distro_source(uri, suite, components, recipe)
    };
    Ok(EntryInspection {
        has_docker_uri: docker,
        active: true,
        approved,
        exact_docker,
    })
}

pub(super) fn inspect_deb822_entry(
    fields: &BTreeMap<String, String>,
    recipe: LinuxRootlessRecipeV1,
) -> anyhow::Result<EntryInspection> {
    for field in [
        "trusted",
        "allow-insecure",
        "allow-weak",
        "allow-downgrade-to-insecure",
        "check-valid-until",
        "valid-until-min",
        "valid-until-max",
        "date-max-future",
    ] {
        anyhow::ensure!(
            !fields.contains_key(field),
            "APT source overrides package signature or freshness verification"
        );
    }
    let uris = values(fields, "uris");
    let docker = uris.iter().any(|uri| uri.contains("download.docker.com"));
    if fields
        .get("enabled")
        .is_some_and(|enabled| enabled.eq_ignore_ascii_case("no"))
    {
        return Ok(EntryInspection {
            has_docker_uri: docker,
            active: false,
            approved: true,
            exact_docker: false,
        });
    }
    if let Some(enabled) = fields.get("enabled") {
        anyhow::ensure!(
            enabled.eq_ignore_ascii_case("yes"),
            "invalid Debian source Enabled field"
        );
    }
    let allowed = [
        "types",
        "uris",
        "suites",
        "components",
        "architectures",
        "signed-by",
        "enabled",
    ];
    for key in fields.keys() {
        let vendor_metadata =
            recipe == LinuxRootlessRecipeV1::PopOs2404Amd64 && key.starts_with("x-repolib-");
        anyhow::ensure!(
            allowed.contains(&key.as_str()) || vendor_metadata,
            "unsupported Debian source field"
        );
    }
    let source_types = values(fields, "types");
    let suites = values(fields, "suites");
    let components = values(fields, "components");
    let signed_by = fields.get("signed-by").map(String::as_str);
    let architecture_is_pinned = fields
        .get("architectures")
        .is_none_or(|architectures| architectures.trim() == "amd64");
    let uri = (uris.len() == 1).then(|| uris[0]);
    let key_is_pinned = uri
        .and_then(|uri| expected_keyring(uri, recipe))
        .is_some_and(|expected| signed_by == Some(expected));
    let exact_docker = docker
        && source_types == ["deb"]
        && uri.is_some_and(|uri| {
            docker_repository(recipe).is_some_and(|(expected_uri, _)| uri == expected_uri)
        })
        && suites == [docker_repository(recipe).map_or("", |(_, suite)| suite)]
        && components == ["stable"]
        && fields
            .get("architectures")
            .is_some_and(|value| value == "amd64")
        && signed_by == Some(DOCKER_KEYRING)
        && fields.keys().all(|key| allowed.contains(&key.as_str()));
    let approved = if docker {
        exact_docker
    } else {
        source_types_valid(&source_types)
            && architecture_is_pinned
            && key_is_pinned
            && uri.is_some_and(|uri| {
                suites.iter().all(|suite| {
                    components.iter().all(|component| {
                        is_distro_source(uri, suite, std::slice::from_ref(component), recipe)
                    })
                })
            })
    };
    Ok(EntryInspection {
        has_docker_uri: docker,
        active: true,
        approved,
        exact_docker,
    })
}

fn values<'a>(fields: &'a BTreeMap<String, String>, key: &str) -> Vec<&'a str> {
    fields
        .get(key)
        .map(|value| value.split_whitespace().collect())
        .unwrap_or_default()
}

fn source_types_valid(types: &[&str]) -> bool {
    !types.is_empty()
        && types
            .iter()
            .all(|value| matches!(*value, "deb" | "deb-src"))
        && types
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            == types.len()
}

fn is_exact_docker_source(
    uri: &str,
    suite: &str,
    components: &[&str],
    options: &BTreeMap<String, String>,
    recipe: LinuxRootlessRecipeV1,
) -> bool {
    docker_repository(recipe).is_some_and(|(expected_uri, expected_suite)| {
        uri == expected_uri
            && suite == expected_suite
            && components == ["stable"]
            && options.len() == 2
            && options.get("arch").is_some_and(|value| value == "amd64")
            && options
                .get("signed-by")
                .is_some_and(|value| value == DOCKER_KEYRING)
    })
}

fn is_signature_override(key: &str) -> bool {
    matches!(
        key,
        "trusted"
            | "allow-insecure"
            | "allow-weak"
            | "allow-downgrade-to-insecure"
            | "check-valid-until"
            | "valid-until-min"
            | "valid-until-max"
            | "date-max-future"
    )
}
