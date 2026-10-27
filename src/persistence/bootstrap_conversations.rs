//! Machine-local histories for context-only setup conversations.
//!
//! These identities and messages are independent of project task chats.

mod identity;
mod storage;
mod store;

#[cfg(test)]
mod tests;

pub use identity::BootstrapConversationIdentityV1;
pub use store::BootstrapConversationStore;
