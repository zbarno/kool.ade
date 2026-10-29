pub(super) fn desktop_artifact_executable(
    working_directory: &str,
    executable_id: &str,
    artifact_paths: &[String],
) -> Result<String, String> {
    let path_executable = crate::project_runtime::is_target_relative_executable_path(executable_id);
    let expected_path = if path_executable {
        let joined = std::path::Path::new(working_directory).join(executable_id);
        joined
            .components()
            .filter_map(|component| match component {
                std::path::Component::Normal(part) => Some(part.to_str()),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
            .map(|parts| parts.join("/"))
            .ok_or_else(|| "desktop executable path is not workspace-relative".to_owned())?
    } else {
        executable_id.to_owned()
    };
    let matches = artifact_paths
        .iter()
        .filter(|path| {
            if path_executable {
                path.as_str() == expected_path
            } else {
                std::path::Path::new(path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    == Some(expected_path.as_str())
            }
        })
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(
            "desktop start executable is not uniquely present in the receipt-approved artifact set"
                .into(),
        );
    }
    Ok(format!("/workspace/{}", matches[0]))
}

#[cfg(test)]
mod tests {
    use super::desktop_artifact_executable;

    #[test]
    fn desktop_target_path_selects_exact_receipt_approved_executable() {
        let artifacts = vec![
            "apps/web/dist/app".to_owned(),
            "apps/other/dist/app".to_owned(),
        ];
        assert_eq!(
            desktop_artifact_executable("apps/web", "dist/app", &artifacts).unwrap(),
            "/workspace/apps/web/dist/app"
        );
        assert!(desktop_artifact_executable("apps/missing", "dist/app", &artifacts).is_err());
    }

    #[test]
    fn desktop_target_logical_tool_keeps_unique_artifact_name_compatibility() {
        let artifacts = vec!["apps/web/dist/approved-app".to_owned()];
        assert_eq!(
            desktop_artifact_executable("apps/web", "approved-app", &artifacts).unwrap(),
            "/workspace/apps/web/dist/approved-app"
        );
    }
}
