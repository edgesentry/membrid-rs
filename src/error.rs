use thiserror::Error;

#[derive(Debug, Error)]
pub enum MembridError {
    #[error("storage error: {0}")]
    Storage(String),

    #[error("embedding error: {0}")]
    Embedding(String),

    #[error("Arrow error: {0}")]
    Arrow(#[from] arrow_schema::ArrowError),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("token budget exceeded: used {used}, budget {budget}")]
    TokenBudgetExceeded { used: usize, budget: usize },

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}

impl MembridError {
    pub fn storage(msg: impl Into<String>) -> Self {
        Self::Storage(msg.into())
    }

    pub fn embedding(msg: impl Into<String>) -> Self {
        Self::Embedding(msg.into())
    }

    pub fn other(msg: impl Into<String>) -> Self {
        Self::Other(msg.into())
    }
}

pub type Result<T> = std::result::Result<T, MembridError>;
