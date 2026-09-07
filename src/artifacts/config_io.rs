//! Markdown round-tripping for `.planner/config.md` (SPECIFICATION.md §4).
//!
//! Both category layouts are accepted on parse:
//! * canonical (app-emitted): `## Stakeholders` → `### <Category>` → bullets
//! * the spec-document example: `# Stakeholders` → `## <Category>` → bullets
//! Extra sections are tolerated; unrecognized prose is dropped on re-serialize
//! (the MVP config is app-managed, edits arrive through the settings dialog).

use crate::domain::{CategoryOwners, CurrentUser, Stakeholders};

/// Parsed view of `.planner/config.md`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlannerConfig {
    pub user: Option<CurrentUser>,
    pub stakeholders: Stakeholders,
}

impl PlannerConfig {
    /// Whether the interviewer's identity is usable for routing.
    pub fn has_routing_identity(&self) -> bool {
        self.user.as_ref().is_some_and(CurrentUser::is_set)
    }
}

/// Deterministic serialization used at bootstrap and after user edits.
pub fn serialize(cfg: &PlannerConfig) -> String {
    let mut s = String::new();
    s.push_str("# Planner Configuration\n");
    if let Some(u) = &cfg.user {
        s.push_str("\n## Current User\n");
        s.push_str(&format!("Name: {}\n", u.name.trim()));
        let groups: Vec<String> =
            u.groups.iter().map(|g| g.trim().to_string()).filter(|g| !g.is_empty()).collect();
        let groups_line = format!("Groups: {}\n", groups.join(", "));
        s.push_str(if groups.is_empty() {
            "Groups:\n"
        } else {
            groups_line.as_str()
        });
    }
    s.push_str("\n## Stakeholders\n");
    for entry in &cfg.stakeholders.entries {
        s.push_str(&format!("\n### {}\n", entry.name));
        if entry.members.is_empty() {
            s.push_str("(no owner configured)\n");
        } else {
            for m in &entry.members {
                s.push_str(&format!("- {m}\n"));
            }
        }
    }
    s
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Zone {
    Top,
    User,
    Stakeholders,
    Unknown,
}

/// Lenient parser. A category is considered "open" once its heading appears
/// in the stakeholders zone, and consumes following `-`/`*` bullets.
pub fn parse(text: &str) -> Result<PlannerConfig, String> {
    let mut cfg = PlannerConfig::default();
    let mut zone = Zone::Top;
    // True when the document IS a stakeholders sheet (H1 titled accordingly):
    // its `## Name` headings then denote categories directly (§4 example).
    let mut stakes_root = false;
    let mut category_open = false;

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(level) = heading_level_opt(line) {
            let name = line.trim_start_matches('#').trim().to_string();
            category_open = false;
            if level == 1 {
                // H1 starts (or restarts) the file context.
                if !name.is_empty() {
                    cfg.stakeholders.entries.retain(|_| false); // reset on repeat H1
                    zone = Zone::Top;
                    stakes_root = name.eq_ignore_ascii_case("stakeholders");
                }
                continue;
            }
            if level == 2 {
                match name.to_ascii_lowercase().as_str() {
                    "current user" => zone = Zone::User,
                    "stakeholders" => zone = Zone::Stakeholders,
                    // §4 example style: `# Stakeholders` → `## Product`
                    _ if zone == Zone::Stakeholders || stakes_root => {
                        zone = Zone::Stakeholders;
                        category_open = open_category(&mut cfg, &name);
                    }
                    _ => zone = Zone::Unknown,
                }
                continue;
            }
            // level >= 3
            if zone == Zone::Stakeholders {
                category_open = open_category(&mut cfg, &name);
            }
            continue;
        }

        if zone == Zone::User {
            let user = cfg
                .user
                .get_or_insert_with(|| CurrentUser::new(String::new(), Vec::new()));
            if let Some(v) = line.strip_prefix("Name:") {
                user.name = v.trim().to_string();
            } else if let Some(v) = line.strip_prefix("Groups:") {
                user.groups = v
                    .split(',')
                    .map(|g| g.trim().to_string())
                    .filter(|g| !g.is_empty())
                    .collect();
            }
        } else if zone == Zone::Stakeholders && category_open {
            if let Some(member) = line
                .strip_prefix('-')
                .or_else(|| line.strip_prefix('*'))
                .map(str::trim)
            {
                if !member.is_empty() && cfg.stakeholders.entries.last_mut().is_some_and(|l| {
                    l.members.len() < 50
                }) {
                    cfg.stakeholders
                        .entries
                        .last_mut()
                        .unwrap()
                        .members
                        .push(member.to_string());
                }
            }
        }
    }

    if cfg.user.as_ref().is_some_and(|u| !u.is_set()) {
        cfg.user = None;
    }
    Ok(cfg)
}

/// Register a new (currently empty) category. Reports whether it opened.
fn open_category(cfg: &mut PlannerConfig, name: &str) -> bool {
    if !name.is_empty() {
        cfg.stakeholders
            .entries
            .push(CategoryOwners::new(name.to_string(), Vec::new()));
        true
    } else {
        false
    }
}

fn heading_level_opt(line: &str) -> Option<usize> {
    if !line.starts_with('#') {
        return None;
    }
    Some(line.chars().take_while(|c| *c == '#').count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_the_canonical_form() {
        let mut cfg = PlannerConfig::default();
        cfg.user = Some(CurrentUser::new("Zach", vec!["Development".into(), "Architecture".into()]));
        cfg.stakeholders = Stakeholders::new(vec![
            CategoryOwners::new("Product", vec!["Zach".into(), "Sarah".into()]),
            CategoryOwners::new("Development", vec!["Alex".into(), "Chris".into()]),
            CategoryOwners::new("QA", vec![]),
        ]);
        let md = serialize(&cfg);
        let back = parse(&md).expect("parse");
        assert_eq!(back, cfg);
    }

    #[test]
    fn parses_the_spec_document_example_shape() {
        // Exact structure from SPECIFICATION.md §4 (H1 + H2 categories).
        let md = "# Stakeholders\n\n## Product\n- Zach\n- Sarah\n\n## Development\n- Alex\n- Chris\n\n## QA\n- Taylor\n\n## InfoSec\n- Morgan\n";
        let cfg = parse(md).expect("parse");
        assert_eq!(cfg.stakeholders.entries.len(), 4);
        assert_eq!(
            cfg.stakeholders.find("product").map(|c| c.members.clone()),
            Some(vec!["Zach".to_string(), "Sarah".to_string()])
        );
        assert!(cfg.user.is_none());
    }

    #[test]
    fn tolerant_of_unknown_sections_and_empty_users() {
        let md = "# Planner Configuration\n\n## Notes\nfreeform\n\n## Stakeholders\n\n### Ops\n(no owner configured)\n";
        let cfg = parse(md).expect("parse");
        assert!(cfg.user.is_none());
        assert_eq!(cfg.stakeholders.entries.len(), 1);
        assert!(cfg.stakeholders.entries[0].members.is_empty());
    }

    #[test]
    fn repeated_h1_starts_fresh() {
        let md = "# A\n## Stakeholders\n### X\n- p\n# B\n## Stakeholders\n### Y\n- q\n";
        let cfg = parse(md).expect("parse");
        let names: Vec<&str> = cfg.stakeholders.iter_categories().collect();
        assert_eq!(names, vec!["Y"]);
    }
}
