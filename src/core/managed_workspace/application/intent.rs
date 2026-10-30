use super::*;

pub(super) fn preserve(
    workspace: &ManagedWorkspace,
    request: &ApplicationCreation,
) -> anyhow::Result<()> {
    use crate::core::planning_work::{self, Work, WorkKind};
    let revision = workspace.store.revision()?;
    let (mut work, revision) = planning_work::load_expected(&workspace.store, &revision)?;
    let uid = request.transaction_id.hyphenated().to_string();
    if let Some(existing) = work.iter().find(|work| work.uid == uid) {
        anyhow::ensure!(
            existing.kind == WorkKind::NewProject && existing.request == request.intent,
            "initial idea identity is already used by different planning work"
        );
        return Ok(());
    }
    let mut initial = Work::new(
        String::new(),
        format!("New Project: {}", request.title),
        request.intent.clone(),
        "Ready to discuss your idea with Man.ager".into(),
    );
    initial.uid = uid;
    initial.key = format!("task:{}", initial.uid);
    initial.kind = WorkKind::NewProject;
    work.push(initial);
    planning_work::save_expected(&workspace.store, &work, &revision)?;
    Ok(())
}
