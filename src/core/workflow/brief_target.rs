/// Judge a brief's declared feature identity (`declared`, de-duplicated)
/// against the feature the batch would actually be stamped with (`stamped`,
/// the active feature id). `feature_dir_exists` tests whether a candidate id
/// has a feature document. Returns an actionable operator message when the
/// two identities cannot both be honored; `None` means the guard passes.
pub fn brief_target_problem(
    declared: &[String],
    stamped: Option<&str>,
    feature_dir_exists: &dyn Fn(&str) -> bool,
) -> Option<String> {
    brief_target_problem_at(declared, stamped, feature_dir_exists, "planning/changes")
}

pub fn brief_target_problem_with_store(
    declared: &[String],
    stamped: Option<&str>,
    feature_dir_exists: &dyn Fn(&str) -> bool,
    store: &crate::artifacts::planning_store::PlanningStore,
) -> Option<String> {
    brief_target_problem_at(
        declared,
        stamped,
        feature_dir_exists,
        &store.git_path(crate::artifacts::planning_store::paths::CHANGES),
    )
}

fn brief_target_problem_at(
    declared: &[String],
    stamped: Option<&str>,
    feature_dir_exists: &dyn Fn(&str) -> bool,
    changes_directory: &str,
) -> Option<String> {
    match declared {
        [] => None,
        [id] if !feature_dir_exists(id) => Some(format!(
            "Brief references unknown feature {id}; record the change specification under {changes_directory} before generating tasks"
        )),
        [id] => match stamped {
            Some(stamped) if *id == stamped => None,
            Some(stamped) => Some(format!(
                "Brief targets {id}, but the active feature is {stamped}; generation stamps every story with {stamped} and freezes its specification — make {id} the active feature (conclude {stamped} first, or reopen {id} if it was concluded) before generating its tasks"
            )),
            None => Some(format!(
                "Brief references feature {id}, but no feature is active"
            )),
        },
        _ => Some(format!(
            "Brief feature name declares more than one feature id ({}); name exactly one",
            declared.join(", ")
        )),
    }
}
