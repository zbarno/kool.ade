use std::{collections::BTreeSet, io::Read, time::Duration};

use reqwest::blocking::{Client, Response};
use reqwest::header::{AUTHORIZATION, HeaderValue};
use serde::Deserialize;
use zeroize::{Zeroize, Zeroizing};

const MODELS_ENDPOINT: &str = "https://api.anthropic.com/v1/models?limit=1000";
const API_VERSION: &str = "2026-10-21";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
const RESPONSE_LIMIT: usize = 256 * 1024;

pub(super) enum Verification {
    Verified(Vec<String>),
    Rejected,
    Unknown,
}

pub(super) fn verify_api_key(key: &Zeroizing<String>, catalog: &[String]) -> Verification {
    verify_api_key_at(MODELS_ENDPOINT, key, catalog)
}

fn verify_api_key_at(endpoint: &str, key: &str, catalog: &[String]) -> Verification {
    let client = match Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(REQUEST_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
    {
        Ok(client) => client,
        Err(_) => return Verification::Unknown,
    };
    let Some(authorization) = authorization_header(key) else {
        return Verification::Unknown;
    };
    let response = client
        .get(endpoint)
        .header(AUTHORIZATION, authorization)
        .header("anthropic-version", API_VERSION)
        .header(reqwest::header::ACCEPT, "application/json")
        .send();
    let Ok(response) = response else {
        return Verification::Unknown;
    };
    match response.status() {
        reqwest::StatusCode::OK => match active_model_ids(response) {
            Some(ids) => Verification::Verified(intersect_catalog(catalog, &ids)),
            None => Verification::Unknown,
        },
        reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN => {
            Verification::Rejected
        }
        _ => Verification::Unknown,
    }
}

fn authorization_header(key: &str) -> Option<HeaderValue> {
    let mut value = Zeroizing::new(Vec::with_capacity(key.len() + "Bearer ".len()));
    value.extend_from_slice(b"Bearer ");
    value.extend_from_slice(key.as_bytes());
    let mut header = HeaderValue::from_bytes(value.as_slice()).ok()?;
    header.set_sensitive(true);
    value.zeroize();
    Some(header)
}

fn active_model_ids(response: Response) -> Option<BTreeSet<String>> {
    if response
        .content_length()
        .is_some_and(|length| length > RESPONSE_LIMIT as u64)
    {
        return None;
    }
    let mut body = Vec::new();
    response
        .take(RESPONSE_LIMIT as u64 + 1)
        .read_to_end(&mut body)
        .ok()?;
    if body.len() > RESPONSE_LIMIT {
        return None;
    }
    let catalog: AnthropicModels = serde_json::from_slice(&body).ok()?;
    if catalog.has_more != Some(false) {
        return None;
    }
    Some(
        catalog
            .data
            .into_iter()
            .filter(|model| model.lifecycle.as_deref() == Some("active"))
            .filter(|model| crate::harness::discovery::is_valid_model_identifier(&model.id))
            .map(|model| model.id)
            .collect(),
    )
}

fn intersect_catalog(catalog: &[String], active_ids: &BTreeSet<String>) -> Vec<String> {
    let mut models = catalog
        .iter()
        .filter_map(|model| {
            let (provider, id) = model.split_once('/')?;
            (provider == "anthropic" && active_ids.contains(id)).then(|| model.clone())
        })
        .collect::<Vec<_>>();
    models.sort();
    models.dedup();
    models
}

#[derive(Deserialize)]
struct AnthropicModels {
    data: Vec<Model>,
    has_more: Option<bool>,
}

#[derive(Deserialize)]
struct Model {
    id: String,
    lifecycle: Option<String>,
}

#[cfg(test)]
#[path = "anthropic/tests.rs"]
mod tests;
