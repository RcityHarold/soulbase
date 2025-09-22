use crate::context::{InterceptContext, ProtoRequest, ProtoResponse, RouteBinding};
use crate::errors::InterceptError;
use crate::policy::RoutePolicy;
use crate::stages::{Stage, StageOutcome};
use async_trait::async_trait;
use sb_auth::prelude::{Action, ResourceUrn};

pub struct RoutePolicyStage {
    policy: RoutePolicy,
}

impl RoutePolicyStage {
    pub fn new(policy: RoutePolicy) -> Self {
        Self { policy }
    }

    fn convert_action(action: &str) -> Action {
        match action {
            "Read" | "read" => Action::Read,
            "Write" | "write" => Action::Write,
            "Invoke" | "invoke" => Action::Invoke,
            "List" | "list" => Action::List,
            "Admin" | "admin" => Action::Admin,
            "Configure" | "configure" => Action::Configure,
            _ => Action::Read,
        }
    }
}

#[async_trait]
impl Stage for RoutePolicyStage {
    async fn handle(
        &self,
        cx: &mut InterceptContext,
        req: &mut dyn ProtoRequest,
        _rsp: &mut dyn ProtoResponse,
    ) -> Result<StageOutcome, InterceptError> {
        let Some(spec) = self.policy.match_http(req.method(), req.path()) else {
            return Err(InterceptError::deny_policy("route not declared"));
        };

        let attrs = if spec.bind.attrs_from_body {
            req.read_json()
                .await
                .unwrap_or_else(|_| serde_json::json!({}))
        } else {
            serde_json::json!({})
        };

        cx.route = Some(RouteBinding {
            resource: ResourceUrn(spec.bind.resource.clone()),
            action: Self::convert_action(&spec.bind.action),
            attrs,
            request_schema: spec.bind.request_schema.clone(),
            response_schema: spec.bind.response_schema.clone(),
        });
        Ok(StageOutcome::Continue)
    }
}
