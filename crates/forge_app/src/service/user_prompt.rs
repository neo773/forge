use anyhow::{Context as _, Result};
use forge_domain::ChatRequest;
use handlebars::Handlebars;
use serde::Serialize;

use super::{PromptService, Service};

impl Service {
    pub fn user_prompt_service() -> impl PromptService {
        Live
    }
}

struct Live;

#[derive(Serialize)]
struct PromptContext {
    task: String,
}

#[async_trait::async_trait]
impl PromptService for Live {
    async fn get(&self, request: &ChatRequest) -> Result<String> {
        if let Some(content) = &request.content {
            let template = include_str!("../prompts/coding/user_task.md");

            let mut hb = Handlebars::new();
            hb.set_strict_mode(true);
            hb.register_escape_fn(|str| str.to_string());

            let ctx = PromptContext { task: content.to_string() };

            Ok(hb
                .render_template(template, &ctx)
                .with_context(|| "Failed to render user task template")?)
        } else {
            return Err(anyhow::anyhow!("no task found in request."));
        }
    }
}

#[cfg(test)]
pub mod tests {

    use super::*;

    #[tokio::test]
    async fn test_render_user_prompt() {
        let request = ChatRequest::new(forge_domain::ModelId::new("gpt-3.5-turbo"))
            .content("read this file content from @foo.txt and @bar.txt");
        let rendered_prompt = Service::user_prompt_service().get(&request).await.unwrap();
        insta::assert_snapshot!(rendered_prompt);
    }

    #[tokio::test]
    async fn test_raise_error_when_content_is_none() {
        let request = ChatRequest::new(forge_domain::ModelId::new("gpt-3.5-turbo"));
        let result = Service::user_prompt_service().get(&request).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "no task found in request.");
    }
}
