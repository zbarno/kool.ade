use crate::harness::DependencyNeed;

mod commands;
pub(super) use commands::{
    command_mentions_package, safe_cargo_restore_command, safe_npm_add_command,
    safe_npm_restore_command, valid_exact_version, valid_package_identity, valid_version,
};

pub(super) fn validate(need: &DependencyNeed) -> anyhow::Result<()> {
    anyhow::ensure!(
        ![Some(need.command.as_str()), Some(need.reason.as_str())]
            .into_iter()
            .chain([
                need.package.as_deref(),
                need.version.as_deref(),
                need.source.as_deref(),
            ])
            .flatten()
            .any(|value| value.contains("?[REDACTED]") || value.contains("#[REDACTED]")),
        "dependency request contains a URL query or fragment that was removed before review"
    );
    anyhow::ensure!(
        !need.command.trim().is_empty()
            && need.command.len() <= 2_048
            && !need.command.chars().any(char::is_control),
        "dependency command is empty or invalid"
    );
    anyhow::ensure!(
        !need.reason.trim().is_empty()
            && need.reason.len() <= 2_048
            && !need.reason.chars().any(char::is_control),
        "dependency reason is empty or invalid"
    );
    for (label, value, limit) in [
        ("package", need.package.as_deref(), 256),
        ("version", need.version.as_deref(), 256),
        ("source", need.source.as_deref(), 2_048),
    ] {
        if let Some(value) = value {
            anyhow::ensure!(
                value.len() <= limit && !value.chars().any(char::is_control),
                "dependency {label} is too large or invalid"
            );
        }
    }
    Ok(())
}

pub(super) fn redact_and_bound(need: &mut DependencyNeed) -> bool {
    let raw_command = need.command.clone();
    let raw_app_metadata_empty =
        need.lockfile_identity.is_none() && need.introduced_packages.is_empty();
    let raw_values_fit = need.command.len() <= 2_048
        && need.reason.len() <= 2_048
        && need.package.as_ref().is_none_or(|value| value.len() <= 256)
        && need.version.as_ref().is_none_or(|value| value.len() <= 256)
        && need
            .source
            .as_ref()
            .is_none_or(|value| value.len() <= 2_048);
    need.command = redact_dependency_value(&need.command);
    need.reason = redact_dependency_value(&need.reason);
    need.package = need.package.as_deref().map(redact_dependency_value);
    need.version = need.version.as_deref().map(redact_dependency_value);
    need.source = need.source.as_deref().map(redact_dependency_value);
    need.lockfile_identity = None;
    need.introduced_packages.clear();
    truncate_on_char_boundary(&mut need.command, 2_048);
    truncate_on_char_boundary(&mut need.reason, 2_048);
    if let Some(value) = need.package.as_mut() {
        truncate_on_char_boundary(value, 256);
    }
    if let Some(value) = need.version.as_mut() {
        truncate_on_char_boundary(value, 256);
    }
    if let Some(value) = need.source.as_mut() {
        truncate_on_char_boundary(value, 2_048);
    }
    raw_app_metadata_empty && raw_values_fit && need.command == raw_command
}

fn redact_dependency_value(value: &str) -> String {
    redact_url_queries(&crate::error::redact_secrets(value))
}

fn redact_url_queries(input: &str) -> String {
    const SCHEMES: [&[u8]; 4] = [b"https://", b"http://", b"ssh://", b"git://"];
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    loop {
        let next = SCHEMES
            .iter()
            .filter_map(|scheme| {
                input.as_bytes()[cursor..]
                    .windows(scheme.len())
                    .position(|window| window.eq_ignore_ascii_case(scheme))
                    .map(|relative| (cursor + relative, scheme.len()))
            })
            .min_by_key(|(start, _)| *start);
        let scheme_less = scheme_less_url_query(input, cursor);
        if let Some((query, marker)) = scheme_less
            && next.is_none_or(|(start, _)| query < start)
        {
            output.push_str(&input[cursor..query]);
            let url_end = input[query..]
                .char_indices()
                .find(|(_, character)| character.is_whitespace() || "\"'`<>".contains(*character))
                .map(|(relative, _)| query + relative)
                .unwrap_or(input.len());
            output.push_str(marker);
            cursor = url_end;
            continue;
        }
        let Some((start, scheme_len)) = next else {
            output.push_str(&input[cursor..]);
            break;
        };
        let url_start = start + scheme_len;
        output.push_str(&input[cursor..url_start]);
        let url_end = input[url_start..]
            .char_indices()
            .find(|(_, character)| character.is_whitespace() || "\"'`<>".contains(*character))
            .map(|(relative, _)| url_start + relative)
            .unwrap_or(input.len());
        let url = &input[url_start..url_end];
        if let Some(index) = url.find(['?', '#']) {
            output.push_str(&url[..index]);
            output.push_str(if url.as_bytes()[index] == b'?' {
                "?[REDACTED]"
            } else {
                "#[REDACTED]"
            });
        } else {
            output.push_str(url);
        }
        cursor = url_end;
    }
    output
}

fn scheme_less_url_query(input: &str, cursor: usize) -> Option<(usize, &'static str)> {
    input[cursor..]
        .char_indices()
        .filter(|(_, character)| matches!(character, '?' | '#'))
        .find_map(|(relative, character)| {
            let query = cursor + relative;
            let prefix = &input[..query];
            let token_start = prefix
                .char_indices()
                .rev()
                .find(|(_, character)| character.is_whitespace() || "\"'`<>".contains(*character))
                .map(|(index, character)| index + character.len_utf8())
                .unwrap_or(0);
            let token = &input[token_start..query];
            if token.contains("://") {
                return None;
            }
            let authority = token.rsplit_once('=').map_or(token, |(_, value)| value);
            let authority = authority
                .trim_start_matches(['(', '['])
                .trim_end_matches([',', ';', ')', ']', '}']);
            url::Url::parse(&format!("https://{authority}"))
                .ok()
                .and_then(|url| url.host_str().map(|_| (query, character)))
                .map(|(query, character)| {
                    (
                        query,
                        if character == '?' {
                            "?[REDACTED]"
                        } else {
                            "#[REDACTED]"
                        },
                    )
                })
        })
}

fn truncate_on_char_boundary(value: &mut String, max_bytes: usize) {
    if value.len() <= max_bytes {
        return;
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
}
