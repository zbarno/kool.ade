//! Local application settings contain only the current human user's identity.

use crate::domain::CurrentUser;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlannerConfig {
    pub user: Option<CurrentUser>,
}

impl PlannerConfig {
    pub fn has_routing_identity(&self) -> bool {
        self.user.as_ref().is_some_and(CurrentUser::is_set)
    }
}

pub fn serialize(config: &PlannerConfig) -> String {
    let mut output = String::from("# Local Application Settings\n");
    if let Some(user) = &config.user {
        output.push_str("\n## Current User\n");
        output.push_str(&format!("Name: {}\n", user.name.trim()));
    }
    output
}

pub fn parse(text: &str) -> Result<PlannerConfig, String> {
    let mut config = PlannerConfig::default();
    let mut in_user = false;
    for raw in text.lines() {
        let line = raw.trim();
        if let Some(level) = heading_level(line) {
            let name = line.trim_start_matches('#').trim();
            in_user = level == 2 && name.eq_ignore_ascii_case("current user");
        } else if in_user && let Some(name) = line.strip_prefix("Name:") {
            config.user = Some(CurrentUser::new(name.trim()));
        }
    }
    if config.user.as_ref().is_some_and(|user| !user.is_set()) {
        config.user = None;
    }
    Ok(config)
}

fn heading_level(line: &str) -> Option<usize> {
    line.starts_with('#').then(|| {
        line.chars()
            .take_while(|character| *character == '#')
            .count()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_local_user_identity_only() {
        let expected = PlannerConfig {
            user: Some(CurrentUser::new("Zach")),
        };
        let actual = parse(&serialize(&expected)).expect("parse");
        assert_eq!(actual, expected);
    }

    #[test]
    fn empty_identity_is_not_persisted_as_a_user() {
        let parsed =
            parse("# Local Application Settings\n\n## Current User\nName:   \n").expect("parse");
        assert_eq!(parsed.user, None);
    }
}
