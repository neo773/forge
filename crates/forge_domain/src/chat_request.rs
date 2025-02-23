use std::collections::HashSet;

use derive_setters::Setters;

use crate::{Attachment, ConversationId};

#[derive(Debug, serde::Deserialize, Clone, Setters)]
#[setters(into, strip_option)]
pub struct ChatRequest {
    pub content: String,
    pub conversation_id: ConversationId,
    pub files: HashSet<Attachment>,
}

impl ChatRequest {
    pub fn new(content: impl ToString, conversation_id: ConversationId) -> Self {
        Self {
            content: content.to_string(),
            conversation_id,
            files: Default::default(),
        }
    }
}
