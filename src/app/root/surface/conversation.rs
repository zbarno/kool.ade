macro_rules! conversation_surface_methods {
    () => {
        fn chat_messages(&self) -> &[ChatMessage] {
            match &self.screen {
                Screen::Connected(p) => p.chat.as_slice(),
                Screen::Welcome => &[],
            }
        }

        fn chat_draft(&mut self) -> &mut String {
            match &mut self.screen {
                Screen::Connected(p) => &mut p.draft,
                Screen::Welcome => &mut self.conn_path,
            }
        }

        fn task_messages(&self, key: &str) -> &[ChatMessage] {
            match &self.screen {
                Screen::Connected(p) => p
                    .task_chats
                    .messages
                    .get(key)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]),
                _ => &[],
            }
        }

        fn task_chat_context(&self, key: &str) -> Option<String> {
            let Screen::Connected(p) = &self.screen else {
                return None;
            };
            let (mut context, _) =
                crate::core::task_conversation::presentation(&p.state, &p.task_documents, key)?;
            if let Some(implementation) = p.implementation_states.get(key) {
                context.push_str(&format!(
                    "\n\nImplementation: {}\n{}",
                    implementation.status,
                    crate::core::context_build::clip(&implementation.detail, 1600)
                ));
            }
            Some(context)
        }

        fn task_draft(&mut self, key: &str) -> Option<&mut String> {
            match &mut self.screen {
                Screen::Connected(p) => Some(p.task_chats.drafts.entry(key.into()).or_default()),
                _ => None,
            }
        }

        fn task_chat_active(&self, key: &str) -> bool {
            matches!(&self.screen, Screen::Connected(p) if p.task_turns.contains_key(key))
        }

        fn task_reply_progress(&self, key: &str) -> Option<&crate::harness::LiveProgress> {
            match &self.screen {
                Screen::Connected(p) => p.task_live.get(key),
                _ => None,
            }
        }

        fn task_chat_error(&self) -> Option<&str> {
            match &self.screen {
                Screen::Connected(p) => p.task_chats.error.as_deref(),
                _ => None,
            }
        }

        fn task_reply_busy(&self) -> bool {
            matches!(&self.screen, Screen::Connected(p) if p.active_turn.is_some())
        }

        fn conversation_busy(&self) -> bool {
            matches!(&self.screen, Screen::Connected(p) if p.active_turn.is_some())
        }
    };
}

pub(super) use conversation_surface_methods;
