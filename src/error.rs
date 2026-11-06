use serde::Serialize;
use std::process::ExitCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

/// One validation or consistency finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Issue {
    pub severity: Severity,
    pub code: String,
    pub path: String,
    pub message: String,
}

impl Issue {
    pub fn error(code: &str, path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            code: code.into(),
            path: path.into(),
            message: message.into(),
        }
    }

    pub fn warning(code: &str, path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            code: code.into(),
            path: path.into(),
            message: message.into(),
        }
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

impl std::fmt::Display for Issue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let level = if self.is_error() { "error" } else { "warning" };
        write!(f, "{level} {} {}: {}", self.code, self.path, self.message)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PhvError {
    /// Fixable by the caller: bad flag, malformed input, missing vault. Exit 2, HTTP 400.
    #[error("{0}")]
    Usage(String),
    /// Exit 2, HTTP 404.
    #[error("not found: {0}")]
    NotFound(String),
    /// The resource fails validation. Exit 1, HTTP 422.
    #[error("validation failed")]
    Invalid(Vec<Issue>),
    /// Broken references, a second Patient, a resource still referenced, or an unsorted file. Exit 1, HTTP 409.
    #[error("conflict")]
    Conflict(Vec<Issue>),
    /// Exit 1, HTTP 500.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl PhvError {
    pub fn exit_code(&self) -> ExitCode {
        match self {
            PhvError::Usage(_) | PhvError::NotFound(_) => ExitCode::from(2),
            _ => ExitCode::from(1),
        }
    }

    pub fn issues(&self) -> &[Issue] {
        match self {
            PhvError::Invalid(i) | PhvError::Conflict(i) => i,
            _ => &[],
        }
    }
}
