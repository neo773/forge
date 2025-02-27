use std::collections::HashSet;

use derive_more::derive::{Display, From};
use derive_setters::Setters;
use serde::{Deserialize, Serialize};
use tracing::debug;

use super::{ToolCallFull, ToolResult};
use crate::{ToolChoice, ToolDefinition};

#[derive(
    Debug, schemars::JsonSchema, serde::Deserialize, serde::Serialize, Clone, PartialEq, Eq, Hash,
)]
pub struct Attachment {
    pub content: String,
    pub path: String,
    pub content_type: ContentType,
}

#[derive(
    Debug,
    strum_macros::EnumIter,
    strum_macros::Display,
    strum_macros::EnumString,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
    Clone,
    PartialEq,
    Eq,
    Hash,
)]
pub enum ImageType {
    #[strum(
        to_string = "jpeg",
        serialize = "jpeg",
        serialize = "jpg",
        ascii_case_insensitive
    )]
    Jpeg,
    #[strum(to_string = "png", ascii_case_insensitive)]
    Png,
    #[strum(
        to_string = "webp",
        serialize = "gif",
        serialize = "webp",
        ascii_case_insensitive
    )]
    Webp,
}

#[derive(
    Debug, schemars::JsonSchema, serde::Deserialize, serde::Serialize, Clone, PartialEq, Eq, Hash,
)]
pub enum ContentType {
    Image(ImageType),
    Text,
}

/// Represents a message being sent to the LLM provider
/// NOTE: ToolResults message are part of the larger Request object and not part
/// of the message.
#[derive(Clone, Debug, Deserialize, From, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextMessage {
    ContentMessage(ContentMessage),
    ToolMessage(ToolResult),
    Attachments(HashSet<Attachment>),
}

impl ContextMessage {
    pub fn user(content: impl ToString) -> Self {
        ContentMessage {
            role: Role::User,
            content: content.to_string(),
            tool_calls: None,
        }
        .into()
    }

    pub fn system(content: impl ToString) -> Self {
        ContentMessage {
            role: Role::System,
            content: content.to_string(),
            tool_calls: None,
        }
        .into()
    }

    pub fn assistant(content: impl ToString, tool_calls: Option<Vec<ToolCallFull>>) -> Self {
        let tool_calls =
            tool_calls.and_then(|calls| if calls.is_empty() { None } else { Some(calls) });
        ContentMessage {
            role: Role::Assistant,
            content: content.to_string(),
            tool_calls,
        }
        .into()
    }

    pub fn tool_result(result: ToolResult) -> Self {
        Self::ToolMessage(result)
    }

    pub fn has_role(&self, role: Role) -> bool {
        match self {
            ContextMessage::ContentMessage(message) => message.role == role,
            ContextMessage::ToolMessage(_) => false,
            ContextMessage::Attachments(_) => Role::User == role,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Setters)]
#[setters(strip_option, into)]
#[serde(rename_all = "snake_case")]
pub struct ContentMessage {
    pub role: Role,
    pub content: String,
    pub tool_calls: Option<Vec<ToolCallFull>>,
}

impl ContentMessage {
    pub fn assistant(content: impl ToString) -> Self {
        Self {
            role: Role::Assistant,
            content: content.to_string(),
            tool_calls: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum Role {
    System,
    User,
    Assistant,
}

/// Represents a request being made to the LLM provider. By default the request
/// is created with assuming the model supports use of external tools.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Setters, Default)]
#[setters(into, strip_option)]
pub struct Context {
    pub messages: Vec<ContextMessage>,
    pub tools: Vec<ToolDefinition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
}

impl Context {
    pub fn add_attachments(mut self, attachment: HashSet<Attachment>) -> Self {
        self.messages.push(ContextMessage::Attachments(attachment));
        self
    }
    pub fn add_tool(mut self, tool: impl Into<ToolDefinition>) -> Self {
        let tool: ToolDefinition = tool.into();
        self.tools.push(tool);
        self
    }

    pub fn add_message(mut self, content: impl Into<ContextMessage>) -> Self {
        let message = content.into();
        debug!(message = ?message, "Adding message to context");
        self.messages.push(message);

        self
    }

    pub fn extend_tools(mut self, tools: Vec<impl Into<ToolDefinition>>) -> Self {
        self.tools.extend(tools.into_iter().map(Into::into));
        self
    }

    pub fn add_tool_results(mut self, results: Vec<ToolResult>) -> Self {
        debug!(results = ?results, "Adding tool results to context");
        self.messages
            .extend(results.into_iter().map(ContextMessage::tool_result));
        self
    }

    /// Updates the set system message
    pub fn set_first_system_message(mut self, content: impl Into<String>) -> Self {
        if self.messages.is_empty() {
            self.add_message(ContextMessage::system(content.into()))
        } else {
            if let Some(ContextMessage::ContentMessage(content_message)) = self.messages.get_mut(0)
            {
                if content_message.role == Role::System {
                    content_message.content = content.into();
                } else {
                    self.messages
                        .insert(0, ContextMessage::system(content.into()));
                }
            }

            self
        }
    }

    /// Converts the context to textual format
    pub fn to_text(&self) -> String {
        let mut lines = String::new();

        for message in self.messages.iter() {
            match message {
                ContextMessage::ContentMessage(message) => {
                    lines.push_str(&format!("<message role=\"{}\">", message.role));
                    lines.push_str(&format!("<content>{}</content>", message.content));
                    if let Some(tool_calls) = &message.tool_calls {
                        for call in tool_calls {
                            lines.push_str(&format!(
                                "<tool_call name=\"{}\"><![CDATA[{}]]></tool_call>",
                                call.name.as_str(),
                                serde_json::to_string(&call.arguments).unwrap()
                            ));
                        }
                    }

                    lines.push_str("</message>");
                }
                ContextMessage::ToolMessage(result) => {
                    lines.push_str("<message role=\"tool\">");

                    lines.push_str(&format!(
                        "<tool_result name=\"{}\"><![CDATA[{}]]></tool_result>",
                        result.name.as_str(),
                        serde_json::to_string(&result.content).unwrap()
                    ));
                    lines.push_str("</message>");
                }
                ContextMessage::Attachments(attachments) => {
                    attachments.iter().for_each(|attachment| {
                        lines.push_str(
                            format!("<file_attachment path=\"{}\">", attachment.path).as_str(),
                        );
                    })
                }
            }
        }

        format!("<chat_history>{}</chat_history>", lines)
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn test_override_system_message() {
        let request = Context::default()
            .add_message(ContextMessage::system("Initial system message"))
            .set_first_system_message("Updated system message");

        assert_eq!(
            request.messages[0],
            ContextMessage::system("Updated system message")
        );
    }

    #[test]
    fn test_set_system_message() {
        let request = Context::default().set_first_system_message("A system message");

        assert_eq!(
            request.messages[0],
            ContextMessage::system("A system message")
        );
    }

    #[test]
    fn test_insert_system_message() {
        let request = Context::default()
            .add_message(ContextMessage::user("Do something"))
            .set_first_system_message("A system message");

        assert_eq!(
            request.messages[0],
            ContextMessage::system("A system message")
        );
    }
}
