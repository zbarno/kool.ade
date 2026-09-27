//! Hard limits for model-selected, read-only repository exploration.
use std::collections::HashMap;

use serde_json::Value;

const MAX_TOOL_CALLS: usize = 80;
const MAX_UNIQUE_PATHS: usize = 40;
const MAX_CALLS_PER_PATH: usize = 12;
const MAX_PROVIDER_RETRIES: usize = 4;

const READ_TOOLS: &[&str] = &["read", "grep", "find", "ls"];

#[derive(Default)]
pub(super) struct PlanningReadBudget {
    calls: usize,
    paths: HashMap<String, usize>,
    retries: usize,
}

impl PlanningReadBudget {
    /// Accounts for a read-only tool start and rejects calls beyond the
    /// per-turn caps before the sandbox executes them.
    pub(super) fn observe_line(&mut self, line: &str) -> Result<(), String> {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            return Ok(());
        };
        match event.get("type").and_then(Value::as_str) {
            Some("auto_retry_start") => {
                self.retries += 1;
                if self.retries > MAX_PROVIDER_RETRIES {
                    return Err(format!(
                        "read-only planning provider retry limit exceeded: attempt {} (limit {MAX_PROVIDER_RETRIES})",
                        self.retries
                    ));
                }
                return Ok(());
            }
            Some("tool_execution_start") => {}
            _ => return Ok(()),
        }
        let tool = event
            .get("toolName")
            .and_then(Value::as_str)
            .unwrap_or("tool");
        if !READ_TOOLS.contains(&tool) {
            return Ok(());
        }

        let path = event
            .pointer("/args/path")
            .and_then(Value::as_str)
            .unwrap_or("<project-root>")
            .to_owned();
        // `find` accepts a target pattern without a directory path. Treat each
        // distinct pattern as its own bounded target; otherwise unrelated
        // repository-wide lookups all consume one `<project-root>` allowance.
        let budget_key = if tool == "find" && path == "<project-root>" {
            event
                .pointer("/args/pattern")
                .and_then(Value::as_str)
                .map(|pattern| format!("{path}:{pattern}"))
                .unwrap_or_else(|| path.clone())
        } else {
            path.clone()
        };
        let path_calls = self.paths.get(&budget_key).copied().unwrap_or_default() + 1;
        if path_calls > MAX_CALLS_PER_PATH {
            return Err(format!(
                "read-only planning path limit exceeded: {tool} requested {path} for the {path_calls}th time (limit {MAX_CALLS_PER_PATH})"
            ));
        }
        if !self.paths.contains_key(&budget_key) && self.paths.len() >= MAX_UNIQUE_PATHS {
            return Err(format!(
                "read-only planning path limit exceeded: request for {path} would exceed {MAX_UNIQUE_PATHS} distinct paths"
            ));
        }
        if self.calls >= MAX_TOOL_CALLS {
            return Err(format!(
                "read-only planning tool-call limit exceeded: {tool} on {path} would exceed {MAX_TOOL_CALLS} calls"
            ));
        }
        self.calls += 1;
        *self.paths.entry(budget_key).or_default() += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(path: Option<&str>) -> String {
        let mut args = serde_json::Map::new();
        if let Some(path) = path {
            args.insert("path".into(), Value::String(path.into()));
        }
        serde_json::json!({"type":"tool_execution_start","toolName":"read","args":args}).to_string()
    }

    #[test]
    fn ignores_non_tool_events_and_tools_outside_the_read_only_set() {
        let mut budget = PlanningReadBudget::default();
        assert!(budget.observe_line(r#"{"type":"message_update"}"#).is_ok());
        assert!(
            budget
                .observe_line(r#"{"type":"tool_execution_start","toolName":"packet_bash"}"#)
                .is_ok()
        );
        assert_eq!(budget.calls, 0);
    }

    #[test]
    fn accepts_existing_live_probe_call_counts_with_repeated_file_reads() {
        let mut budget = PlanningReadBudget::default();
        for i in 0..26 {
            let path = if i < 5 {
                "src/ui/layout.rs".to_owned()
            } else {
                format!("src/ui/{}.rs", (i - 5) % 16)
            };
            budget.observe_line(&read(Some(&path))).unwrap();
        }
        assert_eq!(budget.calls, 26);
        assert_eq!(budget.paths.len(), 17);
    }

    #[test]
    fn rejects_a_thirteenth_read_of_the_same_path() {
        let mut budget = PlanningReadBudget::default();
        for _ in 0..MAX_CALLS_PER_PATH {
            budget.observe_line(&read(Some("src/app/root.rs"))).unwrap();
        }
        let error = budget
            .observe_line(&read(Some("src/app/root.rs")))
            .unwrap_err();
        assert!(error.contains("13th time"));
        assert!(error.contains("src/app/root.rs"));
    }

    #[test]
    fn root_find_patterns_have_independent_bounded_targets() {
        let mut budget = PlanningReadBudget::default();
        let root_find = |pattern: &str| {
            serde_json::json!({
                "type": "tool_execution_start",
                "toolName": "find",
                "args": {"pattern": pattern}
            })
            .to_string()
        };
        for pattern in [
            "src/app/welcome.rs",
            "src/ui/layout.rs",
            "src/core/state.rs",
        ] {
            budget.observe_line(&root_find(pattern)).unwrap();
        }
        assert_eq!(budget.calls, 3);
        assert_eq!(budget.paths.len(), 3);
    }

    #[test]
    fn rejects_a_forty_first_distinct_path() {
        let mut budget = PlanningReadBudget::default();
        for i in 0..MAX_UNIQUE_PATHS {
            let path = format!("src/area/{i}.rs");
            budget.observe_line(&read(Some(&path))).unwrap();
        }
        let error = budget
            .observe_line(&read(Some("src/area/extra.rs")))
            .unwrap_err();
        assert!(error.contains("40 distinct paths"));
    }

    #[test]
    fn rejects_the_eighty_first_read_only_tool_call() {
        let mut budget = PlanningReadBudget::default();
        for i in 0..MAX_TOOL_CALLS {
            let path = format!("src/area/{}.rs", i / MAX_CALLS_PER_PATH);
            budget.observe_line(&read(Some(&path))).unwrap();
        }
        let error = budget
            .observe_line(&read(Some("src/area/10.rs")))
            .unwrap_err();
        assert!(error.contains("would exceed 80 calls"));
    }

    #[test]
    fn rejects_excessive_provider_retry_loops() {
        let mut budget = PlanningReadBudget::default();
        for _ in 0..MAX_PROVIDER_RETRIES {
            budget
                .observe_line(r#"{"type":"auto_retry_start","errorMessage":"terminated"}"#)
                .unwrap();
        }
        let error = budget
            .observe_line(r#"{"type":"auto_retry_start","errorMessage":"terminated"}"#)
            .unwrap_err();
        assert!(error.contains("retry limit exceeded"));
        assert!(error.contains("attempt 5"));
    }
}
