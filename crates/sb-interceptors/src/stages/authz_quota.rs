use crate::context::{InterceptContext, ProtoRequest, ProtoResponse};
use crate::errors::InterceptError;
use crate::stages::{Stage, StageOutcome};
use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use sb_auth::prelude::{AuthContext, AuthFacade};
use sb_errors::prelude::codes;
use sb_types::prelude::Consent;
use serde_json;
use std::sync::Arc;

pub struct AuthzQuotaStage {
    facade: Arc<AuthFacade>,
    pub default_cost: i64,
}

impl AuthzQuotaStage {
    pub fn new(facade: Arc<AuthFacade>) -> Self {
        Self {
            facade,
            default_cost: 1,
        }
    }

    pub fn with_cost(mut self, cost: i64) -> Self {
        self.default_cost = cost.max(0);
        self
    }
}

#[async_trait]
impl Stage for AuthzQuotaStage {
    async fn handle(
        &self,
        cx: &mut InterceptContext,
        _req: &mut dyn ProtoRequest,
        _rsp: &mut dyn ProtoResponse,
    ) -> Result<StageOutcome, InterceptError> {
        let Some(auth_input) = cx.auth_input.clone() else {
            return Err(InterceptError::from_public(
                codes::AUTH_UNAUTHENTICATED,
                "缺少身份凭证。",
            ));
        };
        let Some(route) = cx.route.as_ref() else {
            return Err(InterceptError::deny_policy("route not bound"));
        };

        if let (Some(header_tenant), Some(subject)) =
            (cx.tenant_header.as_ref(), cx.subject.as_ref())
        {
            if &subject.tenant.0 != header_tenant {
                return Err(InterceptError::from_public(
                    codes::AUTH_FORBIDDEN,
                    "租户不匹配。",
                ));
            }
        }

        let consent = parse_consent(cx.consent_token.as_ref())?;

        let ctx = AuthContext {
            input: auth_input,
            resource: route.resource.clone(),
            action: route.action.clone(),
            attrs: route.attrs.clone(),
            consent,
            correlation_id: cx.envelope_seed.correlation_id.clone(),
            cost: self.default_cost,
        };
        let result = self
            .facade
            .authorize(ctx)
            .await
            .map_err(InterceptError::from)?;
        cx.subject = Some(result.subject.clone());
        cx.obligations = result.decision.obligations.clone();
        if !result.decision.allow {
            return Err(InterceptError::from_public(
                codes::AUTH_FORBIDDEN,
                result
                    .decision
                    .reason
                    .unwrap_or_else(|| "请求被拒绝".to_string()),
            ));
        }
        Ok(StageOutcome::Continue)
    }
}

fn parse_consent(token: Option<&String>) -> Result<Option<Consent>, InterceptError> {
    let Some(raw) = token else {
        return Ok(None);
    };
    if raw.trim().is_empty() {
        return Ok(None);
    }
    let decoded = BASE64.decode(raw.trim()).map_err(|err| {
        InterceptError::from_public(codes::AUTH_FORBIDDEN, format!("无效的同意凭证: {err}"))
    })?;
    let consent: Consent = serde_json::from_slice(&decoded).map_err(|err| {
        InterceptError::from_public(codes::AUTH_FORBIDDEN, format!("无法解析同意凭证: {err}"))
    })?;
    Ok(Some(consent))
}
