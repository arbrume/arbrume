//! Error type of the command line and its exit codes.

use std::fmt;
use std::process::ExitCode;

/// An error that ends the command.
#[derive(Debug)]
pub enum CliError {
    /// Invalid arguments, configuration or species file: exit code 2.
    Usage(String),
    /// Generation or writing failed: exit code 1.
    Failed(String),
}

impl CliError {
    /// Process exit code of the error.
    pub fn exit_code(&self) -> ExitCode {
        match self {
            Self::Usage(_) => ExitCode::from(2),
            Self::Failed(_) => ExitCode::from(1),
        }
    }

    /// A usage error from anything printable.
    pub fn usage(message: impl fmt::Display) -> Self {
        Self::Usage(message.to_string())
    }

    /// A failure from anything printable.
    pub fn failed(message: impl fmt::Display) -> Self {
        Self::Failed(message.to_string())
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) | Self::Failed(message) => f.write_str(message),
        }
    }
}
