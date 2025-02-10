use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use colored::Colorize;
use forge_app::{APIService, EnvironmentFactory, Service};
use forge_display::TitleFormat;
use forge_domain::{ChatRequest, ChatResponse, ConversationId, Model, ModelId, Usage};
use forge_tracker::EventKind;
use lazy_static::lazy_static;
use tokio_stream::StreamExt;

use crate::cli::Cli;
use crate::config::Config;
use crate::console::CONSOLE;
use crate::info::Info;
use crate::input::{Console, PromptInput};
use crate::model::{Command, ConfigCommand, UserInput};
use crate::{banner, log};

lazy_static! {
    pub static ref TRACKER: forge_tracker::Tracker = forge_tracker::Tracker::default();
}

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
    models: Option<Vec<Model>>,
    #[allow(dead_code)] // The guard is kept alive by being held in the struct
    _guard: tracing_appender::non_blocking::WorkerGuard,
}

impl UI {
    async fn process_message(&mut self, content: &str) -> Result<()> {
        let model = self
            .config
            .primary_model()
            .map(ModelId::new)
            .unwrap_or(ModelId::from_env(&self.api.environment().await?));

        self.chat(content.to_string(), &model).await
    }

    pub async fn init() -> Result<Self> {
        // Parse CLI arguments first to get flags
        let cli = Cli::parse();

        // Create environment with CLI flags
        let env = EnvironmentFactory::new(std::env::current_dir()?, cli.unrestricted).create()?;
        let guard = log::init_tracing(env.clone())?;
        let config = Config::from(&env);
        let api = Arc::new(Service::api_service(env, cli.system_prompt.clone())?);

        Ok(Self {
            state: Default::default(),
            api: api.clone(),
            config,
            console: Console::new(api.environment().await?),
            cli,
            models: None,
            _guard: guard,
        })
    }

    fn context_reset_message(&self, _: &Command) -> String {
        "All context was cleared, and we're starting fresh. Please re-add files and details so we can get started.".to_string()
            .yellow()
            .bold()
            .to_string()
    }

    pub async fn run(&mut self) -> Result<()> {
        // Handle direct prompt if provided
        let prompt = self.cli.prompt.clone();
        if let Some(prompt) = prompt {
            self.process_message(&prompt).await?;
            return Ok(());
        }

        // Display the banner in dimmed colors since we're in interactive mode
        banner::display()?;

        // Get initial input from file or prompt
        let mut input = match &self.cli.command {
            Some(path) => self.console.upload(path).await?,
            None => self.console.prompt(None).await?,
        };

        // read the model from the config or fallback to environment.
        let mut model = self
            .config
            .primary_model()
            .map(ModelId::new)
            .unwrap_or(ModelId::from_env(&self.api.environment().await?));

        loop {
            match input {
                Command::New => {
                    CONSOLE.writeln(self.context_reset_message(&input))?;
                    self.state = Default::default();
                    input = self.console.prompt(None).await?;
                    continue;
                }
                Command::Reload => {
                    CONSOLE.writeln(self.context_reset_message(&input))?;
                    self.state = Default::default();
                    input = match &self.cli.command {
                        Some(path) => self.console.upload(path).await?,
                        None => self.console.prompt(None).await?,
                    };
                    continue;
                }
                Command::Info => {
                    let info = Info::from(&self.api.environment().await?)
                        .extend(Info::from(&self.state.usage));

                    CONSOLE.writeln(info.to_string())?;

                    let prompt_input = Some((&self.state).into());
                    input = self.console.prompt(prompt_input).await?;
                    continue;
                }
                Command::Message(ref content) => {
                    self.state.current_content = Some(content.clone());
                    if let Err(err) = self.chat(content.clone(), &model).await {
                        CONSOLE.writeln(
                            TitleFormat::failed(format!("{:?}", err))
                                .sub_title(self.state.usage.to_string())
                                .format(),
                        )?;
                    }
                    let prompt_input = Some((&self.state).into());
                    input = self.console.prompt(prompt_input).await?;
                }
                Command::Exit => {
                    break;
                }
                Command::Models => {
                    let models = if let Some(models) = self.models.as_ref() {
                        models
                    } else {
                        let models = self.api.models().await?;
                        self.models = Some(models);
                        self.models.as_ref().unwrap()
                    };
                    let info: Info = models.as_slice().into();
                    CONSOLE.writeln(info.to_string())?;

                    input = self.console.prompt(None).await?;
                }
                Command::Config(config_cmd) => {
                    match config_cmd {
                        ConfigCommand::Set(key, value) => match self.config.insert(&key, &value) {
                            Ok(()) => {
                                model =
                                    self.config.primary_model().map(ModelId::new).unwrap_or(
                                        ModelId::from_env(&self.api.environment().await?),
                                    );
                                CONSOLE.writeln(format!(
                                    "{}: {}",
                                    key.to_string().bold().yellow(),
                                    value.white()
                                ))?;
                            }
                            Err(e) => {
                                CONSOLE.writeln(
                                    TitleFormat::failed(e.to_string())
                                        .sub_title(self.state.usage.to_string())
                                        .format(),
                                )?;
                            }
                        },
                        ConfigCommand::Get(key) => {
                            if let Some(value) = self.config.get(&key) {
                                CONSOLE.writeln(format!(
                                    "{}: {}",
                                    key.to_string().bold().yellow(),
                                    value.white()
                                ))?;
                            } else {
                                CONSOLE.writeln(
                                    TitleFormat::failed(format!("Config key '{}' not found", key))
                                        .sub_title(self.state.usage.to_string())
                                        .format(),
                                )?;
                            }
                        }
                        ConfigCommand::List => {
                            CONSOLE.writeln(Info::from(&self.config).to_string())?;
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
            content: content.clone(),
            model: model.clone(),
            conversation_id: self.state.current_conversation_id,
            custom_instructions: self.cli.custom_instructions.clone(),
        };
        tokio::spawn({
            let content = content.clone();
            async move {
                let _ = TRACKER.dispatch(EventKind::Prompt(content)).await;
            }
        });
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
                        TitleFormat::execute(tool_name.as_str())
                            .sub_title(self.state.usage.to_string())
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
                if !self.cli.verbose {
                    return Ok(());
                }

                let tool_name = tool_result.name.as_str();

                CONSOLE.writeln(format!("{}", tool_result.content.dimmed()))?;

                if tool_result.is_error {
                    CONSOLE.writeln(
                        TitleFormat::failed(tool_name)
                            .sub_title(self.state.usage.to_string())
                            .format(),
                    )?;
                } else {
                    CONSOLE.writeln(
                        TitleFormat::success(tool_name)
                            .sub_title(self.state.usage.to_string())
                            .format(),
                    )?;
                }
            }
            ChatResponse::ConversationStarted(conversation_id) => {
                self.state.current_conversation_id = Some(conversation_id);
            }
            ChatResponse::ModifyContext(_) => {}
            ChatResponse::Complete => {}
            ChatResponse::PartialTitle(_) => {}
            ChatResponse::CompleteTitle(title) => {
                self.state.current_title = Some(title);
            }
            ChatResponse::VariableSet { key, value } => {
                if key == "title" {
                    self.state.current_title = Some(value);
                }
            }
            ChatResponse::FinishReason(_) => {}
            ChatResponse::Usage(u) => {
                self.state.usage = u;
            }
        }
        Ok(())
    }
}
