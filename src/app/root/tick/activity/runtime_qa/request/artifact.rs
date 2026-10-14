use sha2::Digest as _;

pub(super) fn artifact_bundle_sha256(
    paths: &[String],
    hashes: &std::collections::BTreeMap<String, String>,
) -> Result<String, String> {
    let mut entries = paths.to_vec();
    entries.sort();
    if entries.windows(2).any(|pair| pair[0] == pair[1]) || entries.is_empty() {
        return Err("approved QA artifact allowlist is empty or duplicated".into());
    }
    let manifest = entries
        .into_iter()
        .map(|path| {
            let hash = hashes
                .get(&path)
                .ok_or_else(|| "build receipt omits an approved QA artifact".to_owned())?;
            Ok((path, hash.clone()))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let bytes = serde_json::to_vec(&manifest)
        .map_err(|error| format!("artifact bundle manifest cannot be encoded: {error}"))?;
    Ok(format!("{:x}", sha2::Sha256::digest(bytes)))
}

pub(super) fn recipe_sha256(
    recipe: &crate::project_runtime::ExecutionRecipeV1,
) -> Result<String, String> {
    let bytes = serde_json::to_vec(recipe)
        .map_err(|error| format!("approved runtime recipe cannot be encoded: {error}"))?;
    Ok(format!("{:x}", sha2::Sha256::digest(bytes)))
}
