use std::path::PathBuf;

use forge_app::EnvironmentService;
use forge_domain::Environment;

pub struct ForgeEnvironmentService {
    restricted: bool,
}

impl ForgeEnvironmentService {
    /// Creates a new EnvironmentFactory with current working directory
    ///
    /// # Arguments
    /// * `unrestricted` - If true, use unrestricted shell mode (sh/bash) If
    ///   false, use restricted shell mode (rbash)
    pub fn new(restricted: bool) -> Self {
        Self { restricted }
    }

    /// Get path to appropriate shell based on platform and mode
    fn get_shell_path(&self) -> String {
        if cfg!(target_os = "windows") {
            std::env::var("COMSPEC").unwrap_or("cmd.exe".to_string())
        } else if self.restricted {
            // Default to rbash in restricted mode
            "/bin/rbash".to_string()
        } else {
            // Use user's preferred shell or fallback to sh
            std::env::var("SHELL").unwrap_or("/bin/sh".to_string())
        }
    }

    pub fn get(&self) -> Environment {
        dotenv::dotenv().ok();
        let cwd = std::env::current_dir().unwrap_or(PathBuf::from("."));
        let api_key = std::env::var("FORGE_KEY")
            .or_else(|_| std::env::var("OPEN_ROUTER_KEY"))
            .or_else(|_| std::env::var("OPENAI_API_KEY"))
            .or_else(|_| std::env::var("ANTHROPIC_API_KEY"))
            .expect("No API key found. Please set one of: FORGE_KEY, OPEN_ROUTER_KEY, OPENAI_API_KEY, or ANTHROPIC_API_KEY");
        let provider_url =
            std::env::var("FORGE_PROVIDER_URL").expect("FORGE_PROVIDER_URL must be set");

        Environment {
            os: std::env::consts::OS.to_string(),
            cwd,
            shell: self.get_shell_path(),
            base_path: dirs::config_dir()
                .map(|a| a.join("forge"))
                .unwrap_or(PathBuf::from(".").join(".forge")),
            home: dirs::home_dir(),
            api_key,
            provider_url,
        }
    }
}

impl EnvironmentService for ForgeEnvironmentService {
    fn get_environment(&self) -> Environment {
        self.get()
    }
}
