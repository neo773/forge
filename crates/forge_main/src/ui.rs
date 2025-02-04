use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use colored::Colorize;
use forge_app::{APIService, EnvironmentFactory, Service};
use forge_domain::{ChatRequest, ChatResponse, Command, ConversationId, ModelId, Usage, UserInput};
use tokio_stream::StreamExt;

use crate::cli::Cli;
use crate::config::Config;
use crate::console::CONSOLE;
use crate::info::display_info;
use crate::input::{Console, PromptInput};
use crate::status::StatusDisplay;
use crate::{banner, log};

#[derive(Default)]
struct UIState {
    current_conversation_id: Option<ConversationId>,
    current_title: Option<String>,
    current_content: Option<String>,
    usage: Usage,
}

impl From<&UIState> for PromptInput {
    fn from(state: &UIState) -> Self {
        PromptInput::Update {
            title: state.current_title.clone(),
            usage: Some(state.usage.clone()),
        }
    }
}

pub struct UI {
    state: UIState,
    api: Arc<dyn APIService>,
    console: Console,
    cli: Cli,
    config: Config,
    #[allow(dead_code)] // The guard is kept alive by being held in the struct
    _guard: tracing_appender::non_blocking::WorkerGuard,
}

impl UI {
    pub async fn init() -> Result<Self> {
        // NOTE: This has to be first line
        let env = EnvironmentFactory::new(std::env::current_dir()?).create()?;
        let guard = log::init_tracing(env.clone())?;
        let config = Config::from(&env);
        let api = Arc::new(Service::api_service(env)?);

        let cli = Cli::parse();
        Ok(Self {
            state: Default::default(),
            api: api.clone(),
            config,
            console: Console::new(api.environment().await?),
            cli,
            _guard: guard,
        })
    }

    fn context_reset_message(&self, _: &Command) -> String {
        "All context was cleared, and we're starting fresh. Please re-add files and details so we can get started.".to_string()
            .yellow()
            .bold()
            .to_string()
    }

    /// This function is called when the config is updated to perform any necessary actions.
    async fn on_config_update(&self) {
        if let Some(timeout_secs) = self.config.tool_timeout() {
            let timeout = Duration::from_secs(timeout_secs);
            // if we fail, it's okay, we'll just keep the old timeout.
            let _ = self.api.set_tool_timeout(timeout).await;
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        // Display the banner in dimmed colors
        banner::display()?;

        // Get initial input from file or prompt
        let mut input = match &self.cli.prompt {
            Some(path) => self.console.upload(path).await?,
            None => self.console.prompt(None).await?,
        };

        // Initialize tool timeout from config
        if let Some(timeout_secs) = self.config.tool_timeout() {
            self.api
                .set_tool_timeout(Duration::from_secs(timeout_secs))
                .await?;
        }

        loop {
            // read the model from the config or fallback to environment.
            let model = self
                .config
                .primary_model()
                .map(ModelId::new)
                .unwrap_or(ModelId::from_env(&self.api.environment().await?));

            match input {
                Command::End => break,
                Command::New => {
                    CONSOLE.writeln(self.context_reset_message(&input))?;
                    self.state = Default::default();
                    input = self.console.prompt(None).await?;
                    continue;
                }
                Command::Reload => {
                    CONSOLE.writeln(self.context_reset_message(&input))?;
                    self.state = Default::default();
                    input = match &self.cli.prompt {
                        Some(path) => self.console.upload(path).await?,
                        None => self.console.prompt(None).await?,
                    };
                    continue;
                }
                Command::Info => {
                    display_info(&self.api.environment().await?, &self.state.usage)?;
                    let prompt_input = Some((&self.state).into());
                    input = self.console.prompt(prompt_input).await?;
                    continue;
                }
                Command::Message(ref content) => {
                    self.state.current_content = Some(content.clone());
                    if let Err(err) = self.chat(content.clone(), &model).await {
                        CONSOLE.writeln(
                            StatusDisplay::failed(err.to_string(), self.state.usage.clone())
                                .format(),
                        )?;
                    }
                    let prompt_input = Some((&self.state).into());
                    input = self.console.prompt(prompt_input).await?;
                }
                Command::Exit => {
                    break;
                }
                Command::Config { ref key, ref value } => {
                    match (key, value) {
                        (Some(k), Some(v)) => match self.config.insert(k, v) {
                            Ok(()) => {
                                self.on_config_update().await;
                                CONSOLE.writeln(format!("{}: {}", k.bright_blue(), v.green()))?;
                            }
                            Err(e) => {
                                CONSOLE.writeln(format!("{}", e.to_string().bright_red()))?;
                            }
                        },
                        (Some(k), None) => {
                            if let Some(value) = self.config.get(k) {
                                CONSOLE.writeln(format!(
                                    "{}: {}",
                                    k.bright_blue(),
                                    value.green()
                                ))?;
                            } else {
                                CONSOLE.writeln(format!(
                                    "Config key '{}' not found",
                                    k.bright_red()
                                ))?;
                            }
                        }
                        (None, None) => {
                            CONSOLE.writeln(self.config.to_display_string())?;
                        }
                        (None, Some(_)) => {
                            CONSOLE.writeln(format!(
                                "{}",
                                "Error: Cannot set value without a key".bright_red()
                            ))?;
                        }
                    }
                    input = self.console.prompt(None).await?;
                }
            }
        }

        Ok(())
    }

    async fn chat(&mut self, content: String, model: &ModelId) -> Result<()> {
        let chat = ChatRequest {
            content,
            model: model.clone(),
            conversation_id: self.state.current_conversation_id,
            custom_instructions: self.cli.custom_instructions.clone(),
        };
        match self.api.chat(chat).await {
            Ok(mut stream) => self.handle_chat_stream(&mut stream).await,
            Err(err) => Err(err),
        }
    }

    async fn handle_chat_stream(
        &mut self,
        stream: &mut (impl StreamExt<Item = Result<ChatResponse>> + Unpin),
    ) -> Result<()> {
        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    return Ok(());
                }
                maybe_message = stream.next() => {
                    match maybe_message {
                        Some(Ok(message)) => self.handle_chat_response(message)?,
                        Some(Err(err)) => {
                            return Err(err);
                        }
                        None => return Ok(()),
                    }
                }
            }
        }
    }

    fn handle_chat_response(&mut self, message: ChatResponse) -> Result<()> {
        match message {
            ChatResponse::Text(text) => {
                CONSOLE.write(&text)?;
            }
            ChatResponse::ToolCallDetected(tool_name) => {
                if self.cli.verbose {
                    CONSOLE.newline()?;
                    CONSOLE.newline()?;
                    CONSOLE.writeln(
                        StatusDisplay::execute(tool_name.as_str(), self.state.usage.clone())
                            .format(),
                    )?;
                    CONSOLE.newline()?;
                }
            }
            ChatResponse::ToolCallArgPart(arg) => {
                if self.cli.verbose {
                    CONSOLE.write(format!("{}", arg.dimmed()))?;
                }
            }
            ChatResponse::ToolCallStart(_) => {
                CONSOLE.newline()?;
                CONSOLE.newline()?;
            }
            ChatResponse::ToolCallEnd(tool_result) => {
                let tool_name = tool_result.name.as_str();
                // Always show result content for errors, or in verbose mode
                if tool_result.is_error || self.cli.verbose {
                    CONSOLE.writeln(format!("{}", tool_result.content.dimmed()))?;
                }
                let status = if tool_result.is_error {
                    StatusDisplay::failed(tool_name, self.state.usage.clone())
                } else {
                    StatusDisplay::success(tool_name, self.state.usage.clone())
                };

                CONSOLE.writeln(status.format())?;
            }
            ChatResponse::ConversationStarted(conversation_id) => {
                self.state.current_conversation_id = Some(conversation_id);
            }
            ChatResponse::ModifyContext(_) => {}
            ChatResponse::Complete => {}
            ChatResponse::Error(err) => {
                CONSOLE.writeln(
                    StatusDisplay::failed(err.to_string(), self.state.usage.clone()).format(),
                )?;
            }
            ChatResponse::PartialTitle(_) => {}
            ChatResponse::CompleteTitle(title) => {
                self.state.current_title = Some(title);
            }
            ChatResponse::FinishReason(_) => {}
            ChatResponse::Usage(u) => {
                self.state.usage = u;
            }
        }
        Ok(())
    }
}
