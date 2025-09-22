use crate::context::{InterceptContext, ProtoRequest, ProtoResponse};
use crate::errors::InterceptError;
use crate::stages::{Stage, StageOutcome};
use async_trait::async_trait;
use sb_auth::prelude::{Authenticator, AuthnInput};
use sb_errors::prelude::codes;

pub struct AuthnMapStage {
    authenticator: Box<dyn Authenticator>,
}

impl AuthnMapStage {
    pub fn new(authenticator: Box<dyn Authenticator>) -> Self {
        Self { authenticator }
    }
}

#[async_trait]
impl Stage for AuthnMapStage {
    async fn handle(
        &self,
        cx: &mut InterceptContext,
        req: &mut dyn ProtoRequest,
        _rsp: &mut dyn ProtoResponse,
    ) -> Result<StageOutcome, InterceptError> {
        let Some(header) = req.header("Authorization") else {
            return Err(InterceptError::from_public(
                codes::AUTH_UNAUTHENTICATED,
                "请先登录。",
            ));
        };
        let token = header
            .strip_prefix("Bearer ")
            .or_else(|| header.strip_prefix("bearer "))
            .unwrap_or(header.as_str())
            .to_string();
        let input = AuthnInput::Bearer(token);
        let subject = self
            .authenticator
            .authenticate(input.clone())
            .await
            .map_err(InterceptError::from)?;
        cx.subject = Some(subject);
        cx.auth_input = Some(input);
        Ok(StageOutcome::Continue)
    }
}
