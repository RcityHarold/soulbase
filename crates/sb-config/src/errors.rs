use sb_errors::prelude::*;
use std::fmt;

#[derive(Debug)]
pub struct ConfigError(pub ErrorObj);

impl ConfigError {
    pub fn builder(code: ErrorCode) -> ErrorBuilder {
        ErrorBuilder::new(code)
    }

    pub fn into_inner(self) -> ErrorObj {
        self.0
    }
}

impl From<ErrorObj> for ConfigError {
    fn from(value: ErrorObj) -> Self {
        ConfigError(value)
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let view = self.0.to_public();
        write!(f, "{}: {}", view.code, view.message)
    }
}

impl std::error::Error for ConfigError {}

pub fn schema_invalid(phase: &str, detail: &str) -> ConfigError {
    ConfigError(
        ErrorBuilder::new(codes::SCHEMA_VALIDATION_FAILED)
            .user_msg("Configuration is invalid.")
            .dev_msg(format!("{}: {}", phase, detail))
            .build(),
    )
}

pub fn io_provider_unavailable(phase: &str, detail: &str) -> ConfigError {
    ConfigError(
        ErrorBuilder::new(codes::PROVIDER_UNAVAILABLE)
            .user_msg("Configuration source is unavailable.")
            .dev_msg(format!("{}: {}", phase, detail))
            .build(),
    )
}

pub fn auth_forbidden(detail: &str) -> ConfigError {
    ConfigError(
        ErrorBuilder::new(codes::AUTH_FORBIDDEN)
            .user_msg("Forbidden to access configuration source.")
            .dev_msg(detail)
            .build(),
    )
}
