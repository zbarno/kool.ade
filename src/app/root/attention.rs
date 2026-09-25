//! Nonblocking explanation lifecycle for the open task's saved blocker report.
use super::{PacketApp, Screen};
use crate::core::attention::{self, Brief, Controller, View};

pub(super) enum Status {
    Pending(Controller),
    Ready(Brief),
    Error(String),
}

impl PacketApp {
    pub(super) fn poll_attention(&mut self, ctx: &egui::Context) {
        let mut finished = Vec::new();
        let mut pending = false;
        for (path, status) in &self.attention {
            if let Status::Pending(controller) = status
                && let Some(result) = controller.poll()
            {
                finished.push((path.clone(), result));
            } else if matches!(status, Status::Pending(_)) {
                pending = true;
            }
        }
        if pending {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        for (path, result) in finished {
            self.attention.insert(
                path,
                match result {
                    Ok(brief) => Status::Ready(brief),
                    Err(error) => Status::Error(error),
                },
            );
            ctx.request_repaint();
        }
    }

    pub(super) fn attention_view(&mut self, ticket: &str, detail: &str) -> Option<View> {
        #[cfg(test)]
        if let Some(brief) = self.attention_fixture.get(ticket) {
            return Some(View::Ready(brief.clone()));
        }
        let Screen::Connected(project) = &self.screen else {
            return None;
        };
        let report = attention::source_path(&project.state.repo_root, ticket, detail);
        let path = report
            .clone()
            .or_else(|| attention::detail_key(&project.state.repo_root, ticket, detail))?;
        let result = match self.attention.get(&path) {
            Some(Status::Pending(_)) => View::Loading,
            Some(Status::Ready(brief)) => View::Ready(brief.clone()),
            Some(Status::Error(error)) => View::Error(error.clone()),
            None => {
                let controller = if let Some(report_path) = report {
                    Controller::start(project.state.repo_root.clone(), ticket.into(), report_path)
                } else {
                    Controller::start_detail(
                        project.state.repo_root.clone(),
                        ticket.into(),
                        detail.into(),
                    )
                };
                self.attention.insert(path, Status::Pending(controller));
                View::Loading
            }
        };
        Some(result)
    }

    pub(super) fn retry_attention(&mut self, ticket: &str, detail: &str) {
        let Screen::Connected(project) = &self.screen else {
            return;
        };
        let Some(path) = attention::source_path(&project.state.repo_root, ticket, detail)
            .or_else(|| attention::detail_key(&project.state.repo_root, ticket, detail))
        else {
            return;
        };
        self.attention.remove(&path);
        let _ = self.attention_view(ticket, detail);
    }
}
