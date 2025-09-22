use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RetryClass {
    None,
    Transient,
    Permanent,
}

impl RetryClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            RetryClass::None => "none",
            RetryClass::Transient => "transient",
            RetryClass::Permanent => "permanent",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackoffHint {
    pub initial_ms: u64,
    pub max_ms: u64,
}
