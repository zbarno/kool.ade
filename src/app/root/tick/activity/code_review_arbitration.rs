use crate::app::session::Project;

pub(super) fn advance_code_review_arbitration(project: &mut Project) {
    let Some(worker) = project.activity.code_review_arbitration.as_ref() else {
        return;
    };
    for progress in worker.take_progress() {
        project.live_progress.update(progress);
    }
    let result = worker.take_result();
    let Some(result) = result else { return };
    let Some(worker) = project.activity.code_review_arbitration.take() else {
        return;
    };
    let request = worker.request.clone();
    project.live_progress = Default::default();
    crate::app::manager::finish_code_review_arbitration(project, &request, result);
}
