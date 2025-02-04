use std::path::PathBuf;

use async_trait::async_trait;

use crate::error::Result;
use crate::Error;

/// Represents user input types in the chat application.
///
/// This enum encapsulates all forms of input including:
/// - System commands (starting with '/')
/// - Regular chat messages
/// - File content
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// End the current session and exit the application.
    /// This can be triggered with the '/end' command.
    End,
    /// Start a new conversation while preserving history.
    /// This can be triggered with the '/new' command.
    New,
    /// Reload the conversation with the original prompt.
    /// This can be triggered with the '/reload' command.
    Reload,
    /// A regular text message from the user to be processed by the chat system.
    /// Any input that doesn't start with '/' is treated as a message.
    Message(String),
    /// Display system environment information.
    /// This can be triggered with the '/info' command.
    Info,
    /// Exit the application without any further action.
    Exit,
    /// Config command, can be used to get or set or display configuration
    /// values.
    /// Config command for managing application configuration
    Config(ConfigCommand),
}

/// Represents different configuration operations
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigCommand {
    /// List all available configuration options
    List,
    /// Get the value of a specific configuration key
    Get(String),
    /// Set a configuration key to a specific value
    Set(String, String),
}

impl ConfigCommand {
    /// Parse a config command from string arguments
    fn parse(args: &[&str]) -> Result<ConfigCommand> {
        match args.first().copied() {
            None => Ok(ConfigCommand::List),
            Some("set") => {
                if args.len() < 3 {
                    return Err(Error::CommandParse(
                        "Usage: /config set <key> <value>".to_string(),
                    ));
                }
                Ok(ConfigCommand::Set(
                    args[1].to_string(),
                    args[2..].join(" "),
                ))
            }
            Some("get") => {
                if args.len() != 2 {
                    return Err(Error::CommandParse(
                        "Usage: /config get <key>".to_string(),
                    ));
                }
                Ok(ConfigCommand::Get(args[1].to_string()))
            }
            Some(x) => Err(Error::CommandParse(format!(
                "Invalid config subcommand: {}. Use 'set', 'get', or no subcommand to list all options",
                x
            ))),
        }
    }
}

impl Command {
    /// Returns a list of all available command strings.
    ///
    /// These commands are used for:
    /// - Command validation
    /// - Autocompletion
    /// - Help display
    pub fn available_commands() -> Vec<String> {
        vec![
            "/end".to_string(),
            "/new".to_string(),
            "/reload".to_string(),
            "/info".to_string(),
            "/exit".to_string(),
            "/config".to_string(),
            "/config set".to_string(),
            "/config get".to_string(),
            "/models".to_string(),
        ]
    }

    /// Parses a string input into an Input.
    ///
    /// This function:
    /// - Trims whitespace from the input
    /// - Recognizes and validates commands (starting with '/')
    /// - Converts regular text into messages
    ///
    /// # Returns
    /// - `Ok(Input)` - Successfully parsed input
    /// - `Err` - Input was an invalid command
    pub fn parse(input: &str) -> Result<Self> {
        let trimmed = input.trim();

        // Handle config commands
        if trimmed.starts_with("/config") {
            let args: Vec<&str> = trimmed.split_whitespace().skip(1).collect();
            return Ok(Command::Config(ConfigCommand::parse(&args)?));
        }

        match trimmed {
            "/end" => Ok(Command::End),
            "/new" => Ok(Command::New),
            "/reload" => Ok(Command::Reload),
            "/info" => Ok(Command::Info),
            "/exit" => Ok(Command::Exit),
            text => Ok(Command::Message(text.to_string())),
        }
    }
}

/// A trait for handling user input in the application.
///
/// This trait defines the core functionality needed for processing
/// user input, whether it comes from a command line interface,
/// GUI, or file system.
#[async_trait]
pub trait UserInput {
    type PromptInput;
    /// Read content from a file and convert it to the input type.
    ///
    /// # Arguments
    /// * `path` - The path to the file to read
    ///
    /// # Returns
    /// * `Ok(Input)` - Successfully read and parsed file content
    /// * `Err` - Failed to read or parse file
    async fn upload<P: Into<PathBuf> + Send>(&self, path: P) -> anyhow::Result<Command>;

    /// Prompts for user input with optional help text and initial value.
    ///
    /// # Arguments
    /// * `help_text` - Optional help text to display with the prompt
    /// * `initial_text` - Optional initial text to populate the input with
    ///
    /// # Returns
    /// * `Ok(Input)` - Successfully processed input
    /// * `Err` - An error occurred during input processing
    async fn prompt(&self, input: Option<Self::PromptInput>) -> anyhow::Result<Command>;
}
