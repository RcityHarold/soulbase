#[cfg(any(feature = "wrap-reqwest", feature = "wrap-sqlx"))]
use crate::{code::codes, model::ErrorBuilder};

#[cfg(feature = "wrap-reqwest")]
impl From<reqwest::Error> for crate::model::ErrorObj {
    fn from(err: reqwest::Error) -> Self {
        let code = if err.is_timeout() {
            codes::PROVIDER_UNAVAILABLE
        } else if err.is_status() && err.status() == Some(reqwest::StatusCode::TOO_MANY_REQUESTS) {
            codes::QUOTA_RATE_LIMITED
        } else {
            codes::PROVIDER_UNAVAILABLE
        };

        ErrorBuilder::new(code)
            .dev_msg(format!("reqwest: {err}"))
            .meta_kv("provider", serde_json::json!("http"))
            .build()
    }
}

#[cfg(feature = "wrap-sqlx")]
impl From<sqlx::Error> for crate::model::ErrorObj {
    fn from(err: sqlx::Error) -> Self {
        use sqlx::Error::*;
        match err {
            RowNotFound => ErrorBuilder::new(codes::SCHEMA_VALIDATION_FAILED)
                .user_msg("记录未找到。")
                .dev_msg("sqlx::Error::RowNotFound")
                .build(),
            _ => ErrorBuilder::new(codes::PROVIDER_UNAVAILABLE)
                .dev_msg(format!("sqlx: {err}"))
                .meta_kv("provider", serde_json::json!("db"))
                .build(),
        }
    }
}
