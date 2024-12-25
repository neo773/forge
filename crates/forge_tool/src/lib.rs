mod fs;

mod ask;
#[allow(unused)]
mod mcp;
mod outline;
pub mod permission;
mod router;
mod shell;
mod think;
pub mod transport;
mod user_input;

pub use ask::*;
pub use fs::*;
pub use outline::*;
pub use router::*;
pub use shell::*;

#[async_trait::async_trait]
pub trait ToolTrait {
    type Input;
    type Output;

    async fn call(&self, input: Self::Input) -> Result<Self::Output, String>;
}

pub trait Description {
    fn description() -> &'static str;
}
