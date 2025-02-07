use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use async_recursion::async_recursion;
use derive_setters::Setters;
use futures::{Stream, StreamExt};
use schemars::{schema_for, JsonSchema};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::Mutex;

use crate::*;

pub struct AgentMessage<T> {
    pub agent: AgentId,
    pub message: T,
}

#[derive(Setters)]
pub struct Orchestrator {
    provider_svc: Arc<dyn ProviderService>,
    tool_svc: Arc<dyn ToolService>,
    workflow: Workflow,
    system_context: SystemContext,
    state: Arc<Mutex<HashMap<AgentId, Context>>>,
    variables: Arc<Mutex<Variables>>,
    sender: Option<tokio::sync::mpsc::Sender<AgentMessage<anyhow::Result<ChatResponse>>>>,
}

struct ChatCompletionResult {
    pub content: String,
    pub tool_calls: Vec<ToolCallFull>,
}

impl Orchestrator {
    pub fn new(provider: Arc<dyn ProviderService>, tool: Arc<dyn ToolService>) -> Self {
        Self {
            provider_svc: provider,
            tool_svc: tool,
            workflow: Workflow::default(),
            system_context: SystemContext::default(),
            state: Arc::new(Mutex::new(HashMap::new())),
            variables: Arc::new(Mutex::new(Variables::default())),
            sender: None,
        }
    }

    pub async fn agent_context(&self, id: &AgentId) -> Option<Context> {
        let guard = self.state.lock().await;
        guard.get(id).cloned()
    }

    async fn send_message(
        &self,
        agent_id: &AgentId,
        message: anyhow::Result<ChatResponse>,
    ) -> anyhow::Result<()> {
        if let Some(sender) = &self.sender {
            sender
                .send(AgentMessage { agent: agent_id.clone(), message })
                .await?
        }
        Ok(())
    }

    async fn send(&self, agent_id: &AgentId, message: ChatResponse) -> anyhow::Result<()> {
        self.send_message(agent_id, Ok(message)).await
    }

    async fn send_error(&self, agent_id: &AgentId, error: anyhow::Error) -> anyhow::Result<()> {
        self.send_message(agent_id, Err(error)).await
    }

    fn init_default_tool_definitions(&self) -> Vec<ToolDefinition> {
        self.tool_svc.list()
    }

    fn init_tool_definitions(&self, agent: &Agent) -> Vec<ToolDefinition> {
        let allowed = agent.tools.iter().collect::<HashSet<_>>();
        let mut forge_tools = self.init_default_tool_definitions();

        // Adding self to the list of tool definitions
        forge_tools.push(ReadVariable::tool_definition());
        forge_tools.push(WriteVariable::tool_definition());

        forge_tools
            .into_iter()
            .filter(|tool| allowed.contains(&tool.name))
            .collect::<Vec<_>>()
    }

    fn init_agent_context(&self, agent: &Agent, input: &Variables) -> anyhow::Result<Context> {
        let tool_defs = self.init_tool_definitions(agent);

        let tool_usage_prompt = tool_defs.iter().fold("".to_string(), |acc, tool| {
            format!("{}\n{}", acc, tool.usage_prompt())
        });

        let system_message = agent.system_prompt.render(
            &self
                .system_context
                .clone()
                .tool_information(tool_usage_prompt),
        )?;

        let user_message = ContextMessage::user(agent.user_prompt.render(input)?);

        Ok(Context::default()
            .set_first_system_message(system_message)
            .add_message(user_message)
            .extend_tools(tool_defs))
    }

    async fn collect_messages(
        &self,
        response: impl Stream<Item = std::result::Result<ChatCompletionMessage, anyhow::Error>>,
    ) -> anyhow::Result<ChatCompletionResult> {
        let messages = response
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<anyhow::Result<Vec<_>>>()?;

        let content = messages
            .iter()
            .flat_map(|m| m.content.iter())
            .map(|content| content.as_str())
            .collect::<Vec<_>>()
            .join("");

        // From Complete (incase streaming is disabled)
        let mut tool_calls: Vec<ToolCallFull> = messages
            .iter()
            .flat_map(|message| message.tool_call.iter())
            .filter_map(|message| message.as_full().cloned())
            .collect::<Vec<_>>();

        // From partial tool calls
        tool_calls.extend(ToolCallFull::try_from_parts(
            &messages
                .iter()
                .filter_map(|message| message.tool_call.first())
                .clone()
                .filter_map(|tool_call| tool_call.as_partial().cloned())
                .collect::<Vec<_>>(),
        )?);

        // From XML
        tool_calls.extend(ToolCallFull::try_from_xml(&content)?);

        Ok(ChatCompletionResult { content, tool_calls })
    }

    async fn write_variable(
        &self,
        tool_call: &ToolCallFull,
        write: WriteVariable,
    ) -> anyhow::Result<ToolResult> {
        let mut guard = self.variables.lock().await;
        guard.add(write.name.clone(), write.value.clone());
        Ok(ToolResult::from(tool_call.clone())
            .success(format!("Variable {} set to {}", write.name, write.value)))
    }

    async fn read_variable(
        &self,
        tool_call: &ToolCallFull,
        read: ReadVariable,
    ) -> anyhow::Result<ToolResult> {
        let guard = self.variables.lock().await;
        let output = guard.get(&read.name);
        let result = match output {
            Some(value) => {
                ToolResult::from(tool_call.clone()).success(serde_json::to_string(value)?)
            }
            None => ToolResult::from(tool_call.clone())
                .failure(format!("Variable {} not found", read.name)),
        };
        Ok(result)
    }

    #[async_recursion(?Send)]
    async fn execute_tool(&self, tool_call: &ToolCallFull) -> anyhow::Result<Option<ToolResult>> {
        // FIXME: Missing variable set tool call

        if let Some(read) = ReadVariable::parse(tool_call) {
            self.read_variable(tool_call, read).await.map(Some)
        } else if let Some(write) = WriteVariable::parse(tool_call) {
            self.write_variable(tool_call, write).await.map(Some)
        }
        // Check if agent exists
        else if let Some(agent) = self.workflow.find_agent(&tool_call.name.clone().into()) {
            let input = Variables::from(tool_call.arguments.clone());

            // Tools start fresh with no initial context
            self.init_agent(&agent.id, &input).await?;
            Ok(None)
        } else {
            // TODO: Can check if tool exists
            Ok(Some(self.tool_svc.call(tool_call.clone()).await))
        }
    }

    #[async_recursion(?Send)]
    async fn execute_transform(
        &self,
        transforms: &[Transform],
        mut context: Context,
    ) -> anyhow::Result<Context> {
        for transform in transforms.iter() {
            match transform {
                Transform::Assistant {
                    agent_id,
                    token_limit,
                    input: input_key,
                    output: output_key,
                } => {
                    let mut summarize = Summarize::new(&mut context, *token_limit);
                    while let Some(mut summary) = summarize.summarize() {
                        let mut input = Variables::default();
                        input.add(input_key, summary.get());

                        self.init_agent(agent_id, &input).await?;

                        let guard = self.variables.lock().await;

                        let value = guard
                            .get(output_key)
                            .ok_or(Error::UndefinedVariable(output_key.to_string()))?;

                        summary.set(serde_json::to_string(&value)?);
                    }
                }
                Transform::User { agent_id, input: input_key, output: output_key } => {
                    if let Some(ContextMessage::ContentMessage(ContentMessage {
                        role: Role::User,
                        content,
                        ..
                    })) = context.messages.last_mut()
                    {
                        let mut input = Variables::default();
                        input.add(input_key, Value::from(content.clone()));

                        self.init_agent(agent_id, &input).await?;
                        let guard = self.variables.lock().await;
                        let value = guard
                            .get(output_key)
                            .ok_or(Error::UndefinedVariable(output_key.to_string()))?;

                        let message = serde_json::to_string(&value)?;

                        content.push_str(&format!("\n<{output_key}>\n{message}\n</{output_key}>"));
                    }
                }
                Transform::Tap { agent_id, input: input_key } => {
                    let mut input = Variables::default();
                    input.add(input_key, context.to_text());

                    // NOTE: Tap transformers will not modify the context
                    self.init_agent(agent_id, &input).await?;
                }
            }
        }

        Ok(context)
    }

    async fn init_agent(&self, agent: &AgentId, input: &Variables) -> anyhow::Result<()> {
        let agent = self.workflow.get_agent(agent)?;

        let mut context = if agent.ephemeral {
            self.init_agent_context(agent, input)?
        } else {
            self.state
                .lock()
                .await
                .get(&agent.id)
                .cloned()
                .map(Ok)
                .unwrap_or_else(|| self.init_agent_context(agent, input))?
        };

        let content = agent.user_prompt.render(input)?;

        context = context.add_message(ContextMessage::user(content));

        loop {
            context = self.execute_transform(&agent.transforms, context).await?;

            let response = self
                .provider_svc
                .chat(&agent.model, context.clone())
                .await?;
            let ChatCompletionResult { tool_calls, content } =
                self.collect_messages(response).await?;

            let mut tool_results = Vec::new();

            for tool_call in tool_calls.iter() {
                if let Some(result) = self.execute_tool(tool_call).await? {
                    tool_results.push(result);
                }
            }

            context = context
                .add_message(ContextMessage::assistant(content, Some(tool_calls)))
                .add_tool_results(tool_results.clone());

            if !agent.ephemeral {
                self.state
                    .lock()
                    .await
                    .insert(agent.id.clone(), context.clone());
            }

            if tool_results.is_empty() {
                return Ok(());
            }
        }
    }

    pub async fn execute(&self, input: &Variables) -> anyhow::Result<()> {
        let agent = self.workflow.get_head()?;
        self.init_agent(&agent.id, input).await
    }
}

#[derive(Debug, JsonSchema, Deserialize)]
struct ReadVariable {
    name: String,
}

impl ReadVariable {
    fn tool_definition() -> ToolDefinition {
        ToolDefinition {
            name: ToolName::new("forge_read_variable"),
            description: "Reads a global workflow variable".to_string(),
            input_schema: schema_for!(Self),
            output_schema: None,
        }
    }

    fn parse(tool_call: &ToolCallFull) -> Option<Self> {
        if tool_call.name != Self::tool_definition().name {
            return None;
        }
        serde_json::from_value(tool_call.arguments.clone()).ok()
    }
}

impl NamedTool for ReadVariable {
    fn tool_name() -> ToolName {
        Self::tool_definition().name
    }
}

#[derive(Debug, JsonSchema, Deserialize)]
struct WriteVariable {
    name: String,
    value: String,
}

impl WriteVariable {
    fn tool_definition() -> ToolDefinition {
        ToolDefinition {
            name: ToolName::new("forge_write_variable"),
            description: "Writes a global workflow variable".to_string(),
            input_schema: schema_for!(Self),
            output_schema: None,
        }
    }

    fn parse(tool_call: &ToolCallFull) -> Option<Self> {
        if tool_call.name != Self::tool_definition().name {
            return None;
        }
        serde_json::from_value(tool_call.arguments.clone()).ok()
    }
}

impl NamedTool for WriteVariable {
    fn tool_name() -> ToolName {
        Self::tool_definition().name
    }
}
