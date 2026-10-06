use super::*;
use crate::{
    core::implementation::ImplementationStatus,
    domain::{ChatMessage, ChatRole},
};

mod content;
use content::{
    approval_brief, clarification_brief, clarification_detail, event_id, fallback_brief,
    planning_work_brief, planning_work_detail,
};

#[derive(Clone, Copy)]
enum Kind {
    Blocker,
    Approval,
    Clarification,
    PlanningWork,
}

#[derive(Clone, Copy)]
enum Target {
    Task,
    Planning,
    Work,
}

struct Event {
    ticket: String,
    selection: String,
    detail: String,
    kind: Kind,
    target: Target,
    id: String,
}

impl KooladeApp {
    pub(super) fn surface_new_attention(&mut self, ctx: &egui::Context) {
        let open_task =
            ctx.data_mut(|data| data.get_temp::<String>(egui::Id::new("koolade_selected_task")));
        let open_planning = ctx
            .data_mut(|data| data.get_temp::<String>(egui::Id::new("koolade_selected_planning")));
        let open_chat = ctx.data_mut(|data| {
            data.get_temp::<crate::ui::layout::ChatTabs>(egui::Id::new("koolade_chat_tabs"))
                .and_then(|tabs| tabs.active)
        });
        if open_task.is_some() || open_planning.is_some() || open_chat.is_some() {
            return;
        }
        let events = match &self.screen {
            Screen::Connected(project) => {
                let mut events = project
                    .task_documents
                    .iter()
                    .filter_map(|doc| {
                        let state = project.implementation_states.get(&doc.path);
                        let failure = project.queue.blocked.get(&doc.path);
                        let (kind, detail) = if let Some(failure) = failure {
                            (Kind::Blocker, failure.message.clone())
                        } else {
                            let state = state?;
                            match state.status {
                                ImplementationStatus::Blocked
                                | ImplementationStatus::Interrupted => {
                                    (Kind::Blocker, state.detail.clone())
                                }
                                ImplementationStatus::AwaitingApproval => {
                                    (Kind::Approval, state.detail.clone())
                                }
                                _ => return None,
                            }
                        };
                        let id = event_id(&project.state.repo_root, &doc.path, &detail, &kind);
                        Some(Event {
                            ticket: doc.path.clone(),
                            selection: doc.path.clone(),
                            detail,
                            kind,
                            target: Target::Task,
                            id,
                        })
                    })
                    .collect::<Vec<_>>();
                events.extend(
                    crate::core::routing::eligible_items(
                        &project.state.items,
                        &self.cached_user,
                        &project.state.config.stakeholders,
                    )
                    .into_iter()
                    .filter(|item| !item.is_ownership_gap())
                    .map(|item| {
                        let detail = clarification_detail(item);
                        Event {
                            ticket: item.conversation_key().to_owned(),
                            selection: item.id.clone(),
                            id: event_id(
                                &project.state.repo_root,
                                item.conversation_key(),
                                &detail,
                                &Kind::Clarification,
                            ),
                            detail,
                            kind: Kind::Clarification,
                            target: Target::Planning,
                        }
                    }),
                );
                let planning_work =
                    crate::core::planning_work::cards(&project.state, &project.planning_work);
                events.extend(
                    planning_work
                        .into_iter()
                        .filter(|work| {
                            work.status == crate::core::planning_work::WorkStatus::NeedsAttention
                                && project.active_planning_work.as_deref() != Some(&work.key)
                        })
                        .map(|work| {
                            let detail = planning_work_detail(&work);
                            Event {
                                ticket: work.key.clone(),
                                selection: work.key.clone(),
                                id: event_id(
                                    &project.state.repo_root,
                                    &work.key,
                                    &detail,
                                    &Kind::PlanningWork,
                                ),
                                detail,
                                kind: Kind::PlanningWork,
                                target: Target::Work,
                            }
                        }),
                );
                events
            }
            Screen::Welcome => return,
        };

        for event in events {
            let already_saved = match &mut self.screen {
                Screen::Connected(project) => {
                    project.bind_task_conversation_identities();
                    project.task_chats.ensure_loaded(&project.chat_slug);
                    project
                        .task_chats
                        .messages
                        .get(&event.ticket)
                        .is_some_and(|messages| {
                            messages.iter().any(|message| message.id == event.id)
                        })
                }
                Screen::Welcome => return,
            };
            if already_saved {
                continue;
            }

            let brief = match event.kind {
                Kind::Approval => Some(approval_brief(&event.ticket)),
                Kind::Blocker => match self.attention_view(&event.ticket, &event.detail) {
                    Some(View::Ready(brief)) => Some(brief),
                    Some(View::Error(_)) | None => Some(fallback_brief(&event.detail)),
                    Some(View::Loading) => None,
                },
                Kind::Clarification => Some(clarification_brief(&event.detail)),
                Kind::PlanningWork => Some(planning_work_brief(&event.detail)),
            };
            let Some(brief) = brief else { continue };
            let message = ChatMessage {
                id: event.id,
                role: ChatRole::Agent,
                text: brief.conversation_message(),
                ref_item: Some(event.ticket.clone()),
                ts: chrono::Utc::now(),
            };
            if let Screen::Connected(project) = &mut self.screen {
                project.bind_task_conversation_identities();
                project.task_chats.ensure_loaded(&project.chat_slug);
                project.task_chats.remember_response(
                    &project.chat_slug,
                    &event.ticket,
                    vec![message],
                );
            }
            ctx.data_mut(|data| {
                match event.target {
                    Target::Task => {
                        data.remove::<String>(egui::Id::new("koolade_selected_planning"));
                        data.insert_temp(
                            egui::Id::new("koolade_selected_task"),
                            event.selection.clone(),
                        );
                    }
                    Target::Planning => {
                        data.remove::<String>(egui::Id::new("koolade_selected_task"));
                        data.insert_temp(
                            egui::Id::new("koolade_selected_planning"),
                            event.selection.clone(),
                        );
                    }
                    Target::Work => {
                        let mut tabs = data
                            .get_temp::<crate::ui::layout::ChatTabs>(egui::Id::new(
                                "koolade_chat_tabs",
                            ))
                            .unwrap_or_default();
                        tabs.open(&event.selection);
                        data.insert_temp(egui::Id::new("koolade_chat_tabs"), tabs);
                    }
                }
                data.remove::<String>(egui::Id::new("koolade_task_activity"));
            });
            ctx.request_repaint();
            break;
        }
    }
}
