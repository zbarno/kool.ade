use super::{MAX_CATALOG_BYTES, MAX_ENTRIES, ProductCapabilityCatalogV1};

pub(super) fn validate(catalog: &ProductCapabilityCatalogV1) -> Result<(), String> {
    use std::collections::BTreeSet;
    if catalog.schema_version != 1
        || catalog.capabilities.is_empty()
        || catalog.capabilities.len() > MAX_ENTRIES
    {
        return Err("product capability catalog has unsupported schema or size".into());
    }
    let mut ids = BTreeSet::new();
    let mut total_text_bytes = 0usize;
    for item in &catalog.capabilities {
        total_text_bytes = total_text_bytes
            .saturating_add(item.name.len())
            .saturating_add(item.summary.len())
            .saturating_add(item.maturity.len())
            .saturating_add(
                item.terms
                    .iter()
                    .chain(&item.invocation)
                    .chain(&item.constraints)
                    .chain(&item.platform_prerequisites)
                    .chain(&item.provider_prerequisites)
                    .chain(&item.limitations)
                    .chain(&item.implementation_refs)
                    .chain(&item.test_refs)
                    .map(String::len)
                    .sum::<usize>(),
            );
        if !valid_slug(&item.id)
            || !ids.insert(item.id.as_str())
            || item.name.is_empty()
            || item.name.len() > 128
            || item.summary.len() > 1024
            || item.maturity.is_empty()
            || item.maturity.len() > 32
            || item.terms.len() > 32
            || item.invocation.len() > 16
            || item.constraints.len() > 16
            || item.platform_prerequisites.len() > 16
            || item.provider_prerequisites.len() > 16
            || item.limitations.len() > 16
            || item.implementation_refs.len() > 16
            || item.test_refs.len() > 16
            || item
                .implementation_refs
                .iter()
                .chain(&item.test_refs)
                .any(|path| !valid_ref(path))
            || (item
                .readiness_probe_id
                .as_deref()
                .is_some_and(|probe| !valid_slug(probe)))
            || (item.status == super::super::CapabilityStatus::Implemented
                && (item.implementation_refs.is_empty()
                    || item.test_refs.is_empty()
                    || item.maturity != "current"))
            || (item.status == super::super::CapabilityStatus::Planned
                && (item.maturity != "planned" || item.readiness_probe_id.is_some()))
            || (item.status == super::super::CapabilityStatus::Unsupported
                && (item.maturity != "unsupported" || item.readiness_probe_id.is_some()))
        {
            return Err("product capability catalog contains an invalid entry".into());
        }
    }
    if total_text_bytes > MAX_CATALOG_BYTES {
        return Err("product capability catalog text exceeds size limit".into());
    }
    Ok(())
}

fn valid_slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'.'
        })
        && value.as_bytes()[0].is_ascii_lowercase()
}

fn valid_ref(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && !value.starts_with('/')
        && !value.split('/').any(|part| part == ".." || part.is_empty())
}
