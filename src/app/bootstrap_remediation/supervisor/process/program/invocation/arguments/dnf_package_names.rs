/// Package names read from RPM headers for one bounded local transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DnfPackageNames {
    stage: super::dnf_stage::DnfStageId,
    names: Vec<String>,
}

impl DnfPackageNames {
    pub(crate) fn from_rpm_query(
        stage: super::dnf_stage::DnfStageId,
        output: &str,
        expected_count: usize,
    ) -> anyhow::Result<Self> {
        let names = output.lines().map(str::to_owned).collect::<Vec<_>>();
        anyhow::ensure!(
            names.len() == expected_count,
            "RPM query did not return one package name per staged file"
        );
        Self::new(stage, names)
    }

    pub(crate) fn new(
        stage: super::dnf_stage::DnfStageId,
        mut names: Vec<String>,
    ) -> anyhow::Result<Self> {
        stage.validate()?;
        anyhow::ensure!(
            !names.is_empty() && names.len() <= DNF_MAX_STAGED_RPMS,
            "staged RPM package-name count is outside policy"
        );
        anyhow::ensure!(
            names.iter().all(|name| valid_package_name(name)),
            "staged RPM contains an invalid package name"
        );
        names.sort_unstable();
        anyhow::ensure!(
            names.windows(2).all(|pair| pair[0] != pair[1]),
            "staged RPMs contain duplicate package names"
        );
        Ok(Self { stage, names })
    }

    pub(crate) fn validate(&self) -> anyhow::Result<()> {
        Self::new(self.stage.clone(), self.names.clone()).map(|_| ())
    }

    pub(crate) fn as_slice(&self) -> &[String] {
        &self.names
    }

    pub(super) fn csv(&self) -> String {
        self.names.join(",")
    }

    pub(crate) fn stage(&self) -> &super::dnf_stage::DnfStageId {
        &self.stage
    }
}

pub(crate) fn parse_installed_package_names(
    output: &str,
) -> anyhow::Result<std::collections::HashSet<String>> {
    let mut names = std::collections::HashSet::new();
    for name in output.lines() {
        anyhow::ensure!(
            valid_package_name(name),
            "installed RPM query output is malformed"
        );
        names.insert(name.to_owned());
    }
    Ok(names)
}

fn valid_package_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'+' | b'-'))
}

const DNF_MAX_STAGED_RPMS: usize = super::dnf_stage::DNF_MAX_STAGED_RPMS;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rpm_query_names_are_bounded_unique_and_safe_for_setopt() {
        let names = DnfPackageNames::from_rpm_query(
            super::super::dnf_stage::DnfStageId::new(),
            "docker-ce\ncontainerd.io\n",
            2,
        )
        .unwrap();

        assert_eq!(names.as_slice(), ["containerd.io", "docker-ce"]);
        assert!(names.csv().contains(','));
        let stage = super::super::dnf_stage::DnfStageId::new();
        assert!(DnfPackageNames::from_rpm_query(stage.clone(), "pkg\n", 2).is_err());
        assert!(DnfPackageNames::new(stage.clone(), vec!["bad,name".into()]).is_err());
        assert!(DnfPackageNames::new(stage, vec!["same".into(), "same".into()]).is_err());
    }

    #[test]
    fn installed_name_query_is_strict_and_accepts_empty_inventory() {
        assert!(parse_installed_package_names("").unwrap().is_empty());
        assert!(parse_installed_package_names("rpm\nshadow-utils\n").is_ok());
        assert!(parse_installed_package_names("unexpected package\n").is_err());
    }
}
