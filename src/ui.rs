//! Presentation layer (egui). Views draw exclusively through the
//! [`Surface`] trait, so the app state machine (`crate::app`) stays
//! decoupled from pixels.

pub mod chat_pane;
pub mod items_pane;
pub mod layout;
pub mod overlays;
pub mod spec_viewer;
pub mod theme;
pub mod toast;

pub use chat_pane::Intent;
pub use layout::HeaderAction;
pub use toast::ToastQueue;

use crate::domain::chatlog::ChatMessage;
use crate::domain::item::OpenItem;
use crate::domain::user::CurrentUser;

/// Everything the chrome/panes need: read-mostly accessors plus two command
/// sinks (composer intents, header actions).
pub trait Surface {
    // ------- header -------
    fn session_title(&self) -> &str;
    fn is_git_repo(&self) -> bool;
    fn git_branch(&self) -> &str;
    fn git_head(&self) -> &str;
    fn git_dirty(&self) -> bool;
    // ------- chat pane -------
    fn chat_messages(&self) -> &[ChatMessage];
    fn chat_draft(&mut self) -> &mut String;
    fn is_busy(&self) -> bool;
    fn activity_preview(&self) -> Option<&str>;
    // ------- items pane -------
    fn items(&self) -> &[OpenItem];
    fn synthetic_items(&self) -> &[OpenItem];
    fn items_len(&self) -> usize;
    fn current_user(&self) -> &CurrentUser;
    fn next_question_id(&self) -> Option<&str>;
    // ------- spec pane -------
    fn spec_text(&self) -> &str;
    fn spec_words(&self) -> usize;
    // ------- services -------
    fn toasts(&mut self) -> &mut ToastQueue;
    fn on_intent(&mut self, intent: &Intent);
    fn on_header_action(&mut self, action: HeaderAction);
}
