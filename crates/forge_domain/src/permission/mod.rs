mod types;

pub use types::*;

use derive_more::{Display,From};

/// Error type for permission operations
#[derive(From, Debug, Display)]
pub enum PermissionError {
    OperationNotPermitted(String),
}

/// Result type for permission operations
pub type PermissionResult<T> = std::result::Result<T, PermissionError>;
