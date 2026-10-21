use super::*;

impl KooladeApp {
    pub(super) fn runtime_qa_screenshots(
        &self,
        ticket: &str,
        qa_run_id: &str,
        offset: usize,
        limit: usize,
    ) -> Result<crate::ui::task_detail::RuntimeQaScreenshotGalleryView, String> {
        let Screen::Connected(project) = &self.screen else {
            return Err("Connect a workspace to view runtime QA evidence.".into());
        };
        let document = project
            .task_documents
            .iter()
            .find(|document| document.path == ticket)
            .ok_or_else(|| "The task is no longer available.".to_owned())?;
        let planning_uid = document
            .identity
            .as_ref()
            .map(|identity| identity.uid.as_str())
            .ok_or_else(|| "The task has no validated planning identity.".to_owned())?;
        let task = crate::core::lifecycle::LifecycleStore::new(&project.state.planning_store)
            .task_for_planning_item(planning_uid)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "The lifecycle task is no longer available.".to_owned())?;
        let execution = task
            .qa_execution
            .as_ref()
            .filter(|execution| {
                execution.status == crate::core::lifecycle::QaExecutionStatusV1::Completed
            })
            .ok_or_else(|| "This task has no completed runtime QA run.".to_owned())?;
        let request = execution
            .request
            .as_ref()
            .filter(|request| request.qa_run_id == qa_run_id)
            .ok_or_else(|| "The requested QA run does not belong to this task.".to_owned())?;
        let store = crate::runtime_qa::RuntimeQaStore::new(
            project.state.planning_store.root.join("state/runtime-qa"),
        );
        let (persisted_request, evidence, artifacts) =
            store.load_screenshot_gallery(qa_run_id, offset, limit)?;
        if &persisted_request != request || persisted_request.parent_task_uid != task.task_uid {
            return Err("The persisted QA evidence does not match this task.".into());
        }
        let total = evidence.screenshots.len();
        let screenshots = evidence
            .screenshots
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(|metadata| {
                let artifact = artifacts
                    .iter()
                    .find(|artifact| artifact.id == metadata.id)
                    .ok_or_else(|| "A validated QA screenshot is missing.".to_owned())?;
                Ok(crate::ui::task_detail::RuntimeQaScreenshotView {
                    id: metadata.id,
                    scenario_id: metadata.scenario_id,
                    sha256: metadata.sha256,
                    width: metadata.width,
                    height: metadata.height,
                    bytes: artifact.bytes.clone(),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(crate::ui::task_detail::RuntimeQaScreenshotGalleryView { total, screenshots })
    }
}
