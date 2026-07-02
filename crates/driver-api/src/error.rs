use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ErrorCategory {
    SyntaxError,
    PermissionDenied,
    IntegrityConstraintViolation,
    ConnectionFailure,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DatabaseError {
    pub message: String,
    pub sql_state: Option<String>,
    pub position: Option<usize>,
    pub severity: Option<String>,
    pub detail: Option<String>,
    pub category: ErrorCategory,
}

impl DatabaseError {
    pub fn new(message: String) -> Self {
        Self {
            message,
            sql_state: None,
            position: None,
            severity: None,
            detail: None,
            category: ErrorCategory::Unknown,
        }
    }
}

impl From<String> for DatabaseError {
    fn from(message: String) -> Self {
        DatabaseError::new(message)
    }
}

impl From<&str> for DatabaseError {
    fn from(message: &str) -> Self {
        DatabaseError::new(message.to_string())
    }
}

impl std::fmt::Display for DatabaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for DatabaseError {}
