use colored::Colorize;
use forge_domain::Usage;

#[derive(Clone)]
enum Kind {
    Execute,
    Success,
    Failed,
    Task,
}

impl Kind {
    fn icon(&self) -> &'static str {
        match self {
            Kind::Execute => "⚙",
            Kind::Success => "✓",
            Kind::Failed => "✗",
            Kind::Task => "◆",
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Kind::Execute => "EXECUTE",
            Kind::Success => "SUCCESS",
            Kind::Failed => "FAILED",
            Kind::Task => "TASK",
        }
    }
}

#[derive(Clone)]
pub struct StatusDisplay {
    kind: Kind,
    message: String,
    error_details: Option<String>,
    usage: Usage, // (prompt, completion, total)
}

impl StatusDisplay {
    /// Create a status for executing a tool
    pub fn execute(message: impl Into<String>, usage: Usage) -> Self {
        Self {
            kind: Kind::Execute,
            message: message.into(),
            error_details: None,
            usage,
        }
    }

    /// Create a success status
    pub fn success(message: impl Into<String>, usage: Usage) -> Self {
        Self {
            kind: Kind::Success,
            message: message.into(),
            error_details: None,
            usage,
        }
    }

    /// Create a failure status
    pub fn failed(message: impl Into<String>, usage: Usage) -> Self {
        Self {
            kind: Kind::Failed,
            message: message.into(),
            error_details: None,
            usage,
        }
    }

    /// Create a failure status with additional details
    pub fn failed_with(
        message: impl Into<String>,
        details: impl Into<String>,
        usage: Usage,
    ) -> Self {
        Self {
            kind: Kind::Failed,
            message: message.into(),
            error_details: Some(details.into()),
            usage,
        }
    }

    /// Create a task status
    pub fn task(message: impl Into<String>, usage: Usage) -> Self {
        Self {
            kind: Kind::Task,
            message: message.into(),
            error_details: None,
            usage,
        }
    }

    pub fn format(&self) -> String {
        let (icon, label, message) = match self.kind {
            Kind::Execute => (
                self.icon().cyan(),
                self.label().bold().cyan(),
                format!("{} ...", self.message),
            ),
            Kind::Success => (
                self.icon().green(),
                self.label().bold().green(),
                self.message.to_string(),
            ),
            Kind::Failed => {
                let error_suffix = self
                    .error_details
                    .as_ref()
                    .map(|e| format!(" ({})", e))
                    .unwrap_or_default();
                (
                    self.icon().red(),
                    self.label().bold().red(),
                    format!("{}{}", self.message, error_suffix.red()),
                )
            }
            Kind::Task => (
                self.icon().blue(),
                self.label().bold().blue(),
                self.message.to_string(),
            ),
        };

        let timestamp = chrono::Local::now().format("%H:%M:%S%.3f").to_string();
        let mut result = format!("{} {} {} {}", timestamp.dimmed(), icon, label, message);

        result.push_str(
            &format!(
                " [tokens {}/{}/{}]",
                self.usage.prompt_tokens, self.usage.completion_tokens, self.usage.total_tokens
            )
            .dimmed()
            .to_string(),
        );

        result
    }

    fn icon(&self) -> &'static str {
        self.kind.icon()
    }

    fn label(&self) -> &'static str {
        self.kind.label()
    }
}
