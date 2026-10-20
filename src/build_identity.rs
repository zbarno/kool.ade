const BUILD_COMMIT: &str = env!("KOOLADE_BUILD_COMMIT");
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
thread_local! {
    static TEST_IDENTITY: std::cell::Cell<Option<(&'static str, &'static str)>> = const {
        std::cell::Cell::new(None)
    };
}

#[cfg(test)]
#[path = "build_identity/git.rs"]
mod git;

pub(crate) fn current() -> Option<(&'static str, &'static str)> {
    #[cfg(test)]
    if let Some(identity) = TEST_IDENTITY.with(std::cell::Cell::get) {
        return Some(identity);
    }
    (valid_commit(BUILD_COMMIT)).then_some((APP_VERSION, BUILD_COMMIT))
}

#[cfg(test)]
pub(crate) struct TestIdentityGuard(Option<(&'static str, &'static str)>);

#[cfg(test)]
pub(crate) fn override_for_test(version: &'static str, commit: &'static str) -> TestIdentityGuard {
    let previous = TEST_IDENTITY.with(|identity| identity.replace(Some((version, commit))));
    TestIdentityGuard(previous)
}

#[cfg(test)]
impl Drop for TestIdentityGuard {
    fn drop(&mut self) {
        TEST_IDENTITY.with(|identity| identity.set(self.0));
    }
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
