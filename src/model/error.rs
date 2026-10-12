use std::fmt;

/// Process exit codes, the contract every agent can rely on.
pub const BAD_INPUT: i32 = 2;
pub const UNRESOLVABLE: i32 = 3;
pub const PERMISSION: i32 = 4;

/// The one error type the CLI turns into an exit code and a one-line message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub code: i32,
    pub message: String,
}

impl Error {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn bad_input(message: impl Into<String>) -> Self {
        Self::new(BAD_INPUT, message)
    }

    pub fn unresolvable(message: impl Into<String>) -> Self {
        Self::new(UNRESOLVABLE, message)
    }

    pub fn permission(message: impl Into<String>) -> Self {
        Self::new(PERMISSION, message)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
