const BUILD_COMMIT: &str = env!("KOOLADE_BUILD_COMMIT");
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
#[path = "build_identity/git.rs"]
mod git;

pub(crate) fn current() -> Option<(&'static str, &'static str)> {
    (valid_commit(BUILD_COMMIT)).then_some((APP_VERSION, BUILD_COMMIT))
}

fn valid_commit(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::valid_commit;

    #[test]
    fn build_identity_requires_a_full_git_object_id() {
        assert!(valid_commit(&"a".repeat(40)));
        assert!(valid_commit(&"B".repeat(64)));
        assert!(!valid_commit("unknown"));
        assert!(!valid_commit(&"g".repeat(40)));
        assert!(!valid_commit(&"a".repeat(39)));
        assert!(!valid_commit(&"a".repeat(41)));
    }
}
