pub(super) fn navigate_to(ui: &egui::Ui, target_path: String) {
    ui.ctx().data_mut(|data| {
        let selected = egui::Id::new("koolade_selected_task");
        if let Some(current) = data.get_temp::<String>(selected) {
            let history_id = egui::Id::new("koolade_task_relationship_history");
            let mut history = data.get_temp::<Vec<String>>(history_id).unwrap_or_default();
            if history.last() != Some(&current) {
                let current_uid = data
                    .get_temp::<std::collections::HashMap<String, String>>(egui::Id::new(
                        "koolade_task_uids_by_path",
                    ))
                    .and_then(|uids| uids.get(&current).cloned());
                history.push(current);
                let uid_history_id = egui::Id::new("koolade_task_relationship_history_uids");
                let mut uid_history = data
                    .get_temp::<Vec<Option<String>>>(uid_history_id)
                    .unwrap_or_default();
                uid_history.push(current_uid);
                data.insert_temp(uid_history_id, uid_history);
            }
            data.insert_temp(history_id, history);
        }
        data.insert_temp(
            egui::Id::new("koolade_task_relationship_navigation"),
            target_path,
        );
    });
}

pub(super) fn navigate_back(ui: &egui::Ui, previous_path: String) {
    ui.ctx().data_mut(|data| {
        let id = egui::Id::new("koolade_task_relationship_history");
        let mut history = data.get_temp::<Vec<String>>(id).unwrap_or_default();
        history.pop();
        data.insert_temp(id, history);
        let uid_history_id = egui::Id::new("koolade_task_relationship_history_uids");
        let mut uid_history = data
            .get_temp::<Vec<Option<String>>>(uid_history_id)
            .unwrap_or_default();
        let uid = uid_history.pop().flatten();
        let current_path = match uid {
            Some(uid) => data
                .get_temp::<std::collections::HashMap<String, String>>(egui::Id::new(
                    "koolade_task_paths_by_uid",
                ))
                .and_then(|paths| paths.get(&uid).cloned()),
            None => Some(previous_path),
        };
        data.insert_temp(uid_history_id, uid_history);
        if let Some(current_path) = current_path {
            data.insert_temp(
                egui::Id::new("koolade_task_relationship_navigation"),
                current_path,
            );
        }
    });
}

pub(super) fn previous_task_path(ui: &mut egui::Ui) -> Option<String> {
    ui.ctx().data_mut(|data| {
        let history =
            data.get_temp::<Vec<String>>(egui::Id::new("koolade_task_relationship_history"))?;
        let previous = history.last()?.clone();
        let uid = data
            .get_temp::<Vec<Option<String>>>(egui::Id::new(
                "koolade_task_relationship_history_uids",
            ))
            .and_then(|uids| uids.last().cloned());
        match uid {
            Some(Some(uid)) => data
                .get_temp::<std::collections::HashMap<String, String>>(egui::Id::new(
                    "koolade_task_paths_by_uid",
                ))
                .and_then(|paths| paths.get(&uid).cloned()),
            Some(None) | None => Some(previous),
        }
    })
}

pub(super) fn navigate_to_planning(ui: &egui::Ui, key: String) {
    ui.ctx().data_mut(|data| {
        if let Some(current) = data.get_temp::<String>(egui::Id::new("koolade_selected_task")) {
            data.insert_temp(
                egui::Id::new("koolade_task_relationship_work_return"),
                current,
            );
        }
        data.insert_temp(
            egui::Id::new("koolade_task_relationship_planning_navigation"),
            key,
        );
    });
}
