use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    BadInput,
    Unsupported,
    TargetDrift,
    Blocked,
    Timeout,
    Cancelled,
    Resource,
    Protocol,
    Internal,
}

impl ErrorKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ErrorKind::BadInput => "bad_input",
            ErrorKind::Unsupported => "unsupported",
            ErrorKind::TargetDrift => "target_drift",
            ErrorKind::Blocked => "blocked",
            ErrorKind::Timeout => "timeout",
            ErrorKind::Cancelled => "cancelled",
            ErrorKind::Resource => "resource",
            ErrorKind::Protocol => "protocol",
            ErrorKind::Internal => "internal",
        }
    }

    pub fn retryable(&self) -> bool {
        matches!(self, ErrorKind::Timeout | ErrorKind::Blocked | ErrorKind::Resource)
    }
}

impl std::fmt::Display for ErrorKind {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClientError {
    pub kind: ErrorKind,
    pub message: String,
    #[serde(default)]
    pub retryable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op: Option<String>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub detail: Value,
}

impl ClientError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            retryable: kind.retryable(),
            target: None,
            op: None,
            detail: Value::Null,
        }
    }

    pub fn bad_input(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::BadInput, message)
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Unsupported, message)
    }

    pub fn drift(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::TargetDrift, message)
    }

    pub fn blocked(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Blocked, message)
    }

    pub fn timeout(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Timeout, message)
    }

    pub fn cancelled(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Cancelled, message)
    }

    pub fn resource(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Resource, message)
    }

    pub fn protocol(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Protocol, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Internal, message)
    }

    pub fn with_detail(mut self, detail: Value) -> Self {
        self.detail = detail;
        self
    }

    pub fn with_op(mut self, op: impl Into<String>) -> Self {
        self.op = Some(op.into());
        self
    }

    pub fn with_target(mut self, target: impl Into<String>) -> Self {
        self.target = Some(target.into());
        self
    }

    pub fn retry(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (&self.target, &self.op) {
            (Some(target), Some(op)) => write!(out, "{} in {target}.{op}: {}", self.kind, self.message),
            (Some(target), None) => write!(out, "{} in {target}: {}", self.kind, self.message),
            (None, Some(op)) => write!(out, "{} in {op}: {}", self.kind, self.message),
            (None, None) => write!(out, "{}: {}", self.kind, self.message),
        }
    }
}

impl std::error::Error for ClientError {}

impl From<wre_core::error::Error> for ClientError {
    fn from(error: wre_core::error::Error) -> Self {
        ClientError::internal(error.to_string())
    }
}

impl From<serde_json::Error> for ClientError {
    fn from(error: serde_json::Error) -> Self {
        ClientError::bad_input(format!("json rejected: {error}"))
    }
}

impl From<std::io::Error> for ClientError {
    fn from(error: std::io::Error) -> Self {
        ClientError::resource(format!("io failed: {error}"))
    }
}

pub type ClientResult<T> = std::result::Result<T, ClientError>;
