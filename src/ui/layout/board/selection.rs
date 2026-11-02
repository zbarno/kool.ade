use std::collections::HashMap;

pub(super) fn restore(
    ui: &mut egui::Ui,
    selected_path: &mut Option<String>,
    documents: &[crate::artifacts::task_docs::TaskDocument],
    task_paths: &HashMap<String, String>,
    task_uids_by_path: &HashMap<String, String>,
) {
    let selected_uid_id = egui::Id::new("koolade_selected_task_uid");
    let mut selected_uid = ui
        .ctx()
        .data_mut(|data| data.get_temp::<String>(selected_uid_id));
    if let Some(path) = selected_path.as_ref() {
        if let Some(uid) = task_uids_by_path.get(path) {
            selected_uid = Some(uid.clone());
        } else if documents.iter().any(|document| document.path == *path) {
            selected_uid = None;
        } else if let Some(uid) = selected_uid.as_ref() {
            *selected_path = task_paths.get(uid).cloned();
        }
    } else if let Some(uid) = selected_uid.as_ref() {
        *selected_path = task_paths.get(uid).cloned();
    }
    ui.ctx().data_mut(|data| {
        if let Some(path) = selected_path {
            data.insert_temp(egui::Id::new("koolade_selected_task"), path.clone());
        }
        if let Some(uid) = &selected_uid {
            data.insert_temp(selected_uid_id, uid.clone());
        }
        let history_id = egui::Id::new("koolade_task_relationship_history");
        let uid_history_id = egui::Id::new("koolade_task_relationship_history_uids");
        let mut history = data.get_temp::<Vec<String>>(history_id).unwrap_or_default();
        let uid_history = data
            .get_temp::<Vec<Option<String>>>(uid_history_id)
            .unwrap_or_default();
        for (index, uid) in uid_history.iter().enumerate() {
            if let Some(uid) = uid
                && let Some(path) = task_paths.get(uid)
                && let Some(entry) = history.get_mut(index)
            {
                *entry = path.clone();
            }
        }
        data.insert_temp(history_id, history);
    });
}

pub(super) fn persist(
    ui: &mut egui::Ui,
    selected_path: Option<String>,
    task_uids_by_path: &HashMap<String, String>,
) {
    ui.ctx().data_mut(|data| {
        let selected_id = egui::Id::new("koolade_selected_task");
        let selected_uid_id = egui::Id::new("koolade_selected_task_uid");
        if let Some(path) = selected_path {
            if let Some(uid) = task_uids_by_path.get(&path) {
                data.insert_temp(selected_uid_id, uid.clone());
            }
            data.insert_temp(selected_id, path);
        } else {
            data.remove::<String>(selected_id);
            data.remove::<String>(selected_uid_id);
        }
    });
}
