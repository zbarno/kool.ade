use super::*;
use crate::{
    core::implementation::ImplementationStatus,
    domain::{ChatMessage, ChatRole},
};

mod content;
use content::{
    approval_brief, clarification_brief, clarification_detail, clarification_event_id, event_id,
    fallback_brief,
};

#[derive(Clone, Copy)]
enum Kind {
    Blocker,
    Approval,
    Clarification,
}

#[derive(Clone, Copy)]
enum Target {
    Task,
    Planning,
}

struct Event {
    ticket: String,
    selection: String,
    detail: String,
    kind: Kind,
    target: Target,
    id: String,
    clarification_item: Option<crate::domain::OpenItem>,
}

impl KooladeApp {
    pub(super) fn surface_new_attention(&mut self, ctx: &egui::Context) {
        let open_task =
            ctx.data_mut(|data| data.get_temp::<String>(egui::Id::new("koolade_selected_task")));
        let open_planning = ctx
            .data_mut(|data| data.get_temp::<String>(egui::Id::new("koolade_selected_planning")));
        if open_task.is_some() || open_planning.is_some() {
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
                            if !matches!(
                                failure.recovery,
                                crate::core::implementation::RecoveryDisposition::UserAction
                                    | crate::core::implementation::RecoveryDisposition::ExplicitResume
                            ) {
                                return None;
                            }
                            (Kind::Blocker, failure.message.clone())
                        } else {
                            let state = state?;
                            match state.status {
                                ImplementationStatus::AwaitingApproval => {
                                    (Kind::Approval, state.detail.clone())
                                }
                                ImplementationStatus::Blocked
                                | ImplementationStatus::Interrupted => {
                                    (Kind::Blocker, state.detail.clone())
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
                            clarification_item: None,
                        })
                    })
                    .collect::<Vec<_>>();
                events.extend(
                    crate::core::routing::human_actionable_items(
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
                            id: clarification_event_id(item),
                            detail,
                            kind: Kind::Clarification,
                            target: Target::Planning,
                            clarification_item: Some(item.clone()),
                        }
                    }),
                );
                events
            }
            Screen::Welcome => return,
        };

        for event in events {
            let already_handled = |project: &crate::app::session::Project| {
                project.task_chats.has_ambiguous_identity(&event.ticket)
                    || project.task_turns.contains_key(&event.ticket)
                    || project.task_chats.has_failed_task_reply(&event.ticket)
                    || project.task_chats.has_pending_user_reply(&event.ticket)
                    || project
                        .task_chats
                        .messages
                        .get(&event.ticket)
                        .into_iter()
                        .flatten()
                        .any(|message| message.id == event.id)
            };
            let already_saved = match &mut self.screen {
                Screen::Connected(project) => {
                    project.bind_task_conversation_identities();
                    project.task_chats.ensure_loaded(&project.chat_slug);
                    if already_handled(project) {
                        true
                    } else {
                        // Read the peer window's latest saved reply before
                        // creating a proactive question for this conversation.
                        project.task_chats.refresh_now(&project.chat_slug);
                        already_handled(project)
                    }
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
                Kind::Clarification => event
                    .clarification_item
                    .as_ref()
                    .and_then(clarification_brief),
            };
            let Some(brief) = brief else { continue };
            let message = ChatMessage {
                id: event.id,
                role: ChatRole::Agent,
                text: brief.conversation_message(),
                ref_item: Some(event.selection.clone()),
                lease_renewed_at: None,
                answer_context: None,
                clarification: false,
                ts: chrono::Utc::now(),
            };
            let saved = match &mut self.screen {
                Screen::Connected(project) => {
                    project.bind_task_conversation_identities();
                    project.task_chats.ensure_loaded(&project.chat_slug);
                    match project
                        .task_chats
                        .remember_proactive_response_if_unanswered(
                            &project.chat_slug,
                            &event.ticket,
                            vec![message],
                        ) {
                        Ok(saved) => saved,
                        Err(error) => {
                            project.task_chats.error =
                                Some(format!("Proactive task prompt could not be saved: {error}"));
                            false
                        }
                    }
                }
                Screen::Welcome => return,
            };
            if !saved {
                continue;
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
                }
                data.remove::<String>(egui::Id::new("koolade_task_activity"));
            });
            ctx.request_repaint();
            break;
        }
    }
}
