const MAX_ARTIFACT_INPUT_BYTES: usize = 32 * 1024;

pub(super) fn parse(input: &str) -> Result<Vec<String>, String> {
    if input.len() > MAX_ARTIFACT_INPUT_BYTES {
        return Err("Expected artifact paths exceed the input size limit.".into());
    }
    let paths = input
        .lines()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if paths.len() > 128 {
        return Err("Recipes may declare at most 128 expected artifact paths.".into());
    }
    if !paths.is_empty() {
        crate::project_runtime::validate_custom_artifact_paths(&paths).map_err(str::to_owned)?;
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn parses_empty_and_canonical_relative_artifact_paths() {
        assert_eq!(parse(" \n\t\n").unwrap(), Vec::<String>::new());
        assert_eq!(
            parse("dist/index.html\nassets/app.js\n").unwrap(),
            vec!["dist/index.html", "assets/app.js"]
        );
    }

    #[test]
    fn rejects_unsafe_duplicate_and_oversized_artifact_paths() {
        assert!(parse("../outside").is_err());
        assert!(parse("dist/index.html\ndist/index.html").is_err());
        assert!(parse("dist\\index.html").is_err());
        assert!(parse(&"artifact\n".repeat(129)).is_err());
        assert!(parse(&"x".repeat(32 * 1024 + 1)).is_err());
    }
}
