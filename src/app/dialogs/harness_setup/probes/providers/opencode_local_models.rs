use std::{collections::BTreeMap, path::Path};

mod catalog;
mod config;
mod managed;

#[cfg(test)]
#[path = "opencode_local_models/tests.rs"]
mod tests;

#[derive(Debug, Eq, PartialEq)]
pub(super) enum LocalModelEvidence {
    NotConfigured,
    EndpointUnavailable,
    AuthenticationRequired,
    FreshCatalog(Vec<String>),
}

pub(super) fn probe_local_models(models: &[String]) -> LocalModelEvidence {
    let Ok(discovery_directory) = std::env::current_dir() else {
        return LocalModelEvidence::NotConfigured;
    };
    probe_local_models_in(models, &discovery_directory)
}

fn probe_local_models_in(models: &[String], discovery_directory: &Path) -> LocalModelEvidence {
    if std::env::var_os("OPENCODE_CONFIG").is_some()
        || std::env::var_os("OPENCODE_CONFIG_CONTENT").is_some()
        || config::has_project_config(discovery_directory) != Some(false)
    {
        return LocalModelEvidence::NotConfigured;
    }
    let Some(endpoints) = config::effective_provider_endpoints() else {
        return LocalModelEvidence::NotConfigured;
    };
    let local = endpoints
        .into_iter()
        .filter(|(_, endpoint)| endpoint.loopback_configured && endpoint.no_auth_required)
        .collect::<BTreeMap<_, _>>();
    if local.is_empty() {
        return LocalModelEvidence::NotConfigured;
    }

    let mut catalog_available = false;
    let mut authentication_required = false;
    let mut available_models = Vec::new();
    for (provider, endpoint) in local {
        let Some(catalog_url) = endpoint.catalog_url.as_ref() else {
            continue;
        };
        match catalog::probe_model_catalog(catalog_url) {
            catalog::ModelCatalogProbe::Available(catalog) => {
                catalog_available = true;
                available_models.extend(models.iter().filter_map(|model| {
                    let (model_provider, model_id) = model.split_once('/')?;
                    (model_provider == provider.as_str()
                        && (catalog.contains(model_id) || catalog.contains(model)))
                    .then(|| model.to_owned())
                }));
            }
            catalog::ModelCatalogProbe::AuthenticationRequired => {
                authentication_required = true;
            }
            catalog::ModelCatalogProbe::Unavailable => {}
        }
    }
    if catalog_available {
        LocalModelEvidence::FreshCatalog(available_models)
    } else if authentication_required {
        LocalModelEvidence::AuthenticationRequired
    } else {
        LocalModelEvidence::EndpointUnavailable
    }
}
