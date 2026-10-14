use serde::{Deserialize, Serialize};

use crate::project_runtime::recipe::{valid_executable_id, valid_id};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReadinessProbeV1 {
    ProcessAlive {
        executable_id: String,
    },
    WindowVisible {
        title_pattern: String,
    },
    HttpOnApprovedPort {
        port: u16,
        path: String,
        expected_status: u16,
    },
    LocalServiceHealth {
        port: u16,
        path: String,
    },
    CustomApprovedProbe {
        recipe_id: String,
    },
}

impl ReadinessProbeV1 {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::ProcessAlive { executable_id } if !valid_executable_id(executable_id) => {
                Err("process readiness probe has an invalid executable ID".into())
            }
            Self::WindowVisible { title_pattern }
                if title_pattern.trim().is_empty()
                    || title_pattern.len() > 256
                    || title_pattern.chars().any(char::is_control) =>
            {
                Err("window readiness probe title is invalid".into())
            }
            Self::HttpOnApprovedPort {
                port,
                path,
                expected_status,
            } if *port == 0 || !valid_http_path(path) || !(100..=599).contains(expected_status) => {
                Err("HTTP readiness probe is invalid".into())
            }
            Self::LocalServiceHealth { port, path } if *port == 0 || !valid_http_path(path) => {
                Err("local-service readiness probe is invalid".into())
            }
            Self::CustomApprovedProbe { recipe_id } if !valid_id(recipe_id) => {
                Err("custom readiness probe references an invalid recipe".into())
            }
            _ => Ok(()),
        }
    }
}

fn valid_http_path(path: &str) -> bool {
    path.starts_with('/')
        && !path.starts_with("//")
        && path.len() <= 1024
        && !path.contains('\\')
        && !path.chars().any(char::is_control)
        && !path.split('/').any(|part| part == "..")
}
