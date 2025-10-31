use crate::config::GatewayConfig;
use sb_errors::prelude::codes;
use sb_interceptors::context::ProtoRequest;
use sb_interceptors::errors::InterceptError;
use sb_interceptors::prelude::InterceptContext;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tracing::debug;

#[derive(Clone)]
pub struct GatewayService {
    _config: GatewayConfig,
}

impl GatewayService {
    pub fn new(config: GatewayConfig) -> Self {
        Self { _config: config }
    }

    pub async fn handle_tools_execute(
        self: Arc<Self>,
        tenant_id: u64,
        cx: &mut InterceptContext,
        req: &mut dyn ProtoRequest,
    ) -> Result<Value, InterceptError> {
        let body = req.read_json().await?;
        let parsed: ToolExecuteRequest = parse_body(body)?;
        let first_node = parsed
            .plan
            .nodes
            .first()
            .ok_or_else(|| schema_error("工具计划中缺少节点"))?;

        let call_id = first_node.id.clone();
        let route_fragment = format!("tool-route-{}", call_id);
        let payload = json!({
            "invocation": {
                "tool_id": first_node.tool_id,
                "call_id": call_id,
                "input": first_node.input.clone(),
                "strategy": parsed.plan.barrier.mode.clone()
            },
            "result": {
                "tool_id": first_node.tool_id,
                "call_id": route_fragment,
                "success": true,
                "output": json!({
                    "message": "工具执行由 sb-gateway 模拟完成",
                    "node_id": first_node.id,
                    "tenant_id": tenant_id
                }),
                "error": Value::Null,
                "degradation_reason": Value::Null
            },
            "route_id": route_fragment,
            "attempt": 1,
            "latency_ms": 12,
            "evidence": Value::Array(vec![]),
            "output_digest_sha256": Value::Null,
            "blob_ref": Value::Null,
            "degradation_reason": Value::Null,
            "attributes": json!({
                "gateway": "sb-gateway",
                "mode": parsed.plan.barrier.mode,
                "stub": true
            }),
            "metadata": json!({
                "tool_node_id": first_node.id,
                "tool_version": first_node.version,
                "stub": true
            }),
            "manifest": json!({
                "provider": "sb-gateway",
                "scenario": parsed.cycle.get("lane").cloned().unwrap_or(Value::String("unknown".into())),
                "router_digest": parsed.router.get("decision_router_digest").cloned().unwrap_or(Value::Null)
            }),
            "awareness": [
                json!({
                    "event_type": "tool_called",
                    "payload": {
                        "tool_id": first_node.tool_id,
                        "call_id": route_fragment
                    },
                    "degradation_reason": Value::Null,
                    "barrier_id": parsed.plan.barrier.mode
                }),
                json!({
                    "event_type": "tool_responded",
                    "payload": {
                        "tool_id": first_node.tool_id,
                        "status": "ok"
                    },
                    "degradation_reason": Value::Null,
                    "barrier_id": parsed.plan.barrier.mode
                })
            ]
        });

        cx.response_status = Some(200);
        debug!(
            "tool execute tenant={} node={}",
            tenant_id, first_node.id
        );
        Ok(success_envelope(payload, &cx.request_id))
    }

    pub async fn handle_collab_execute(
        self: Arc<Self>,
        tenant_id: u64,
        cx: &mut InterceptContext,
        req: &mut dyn ProtoRequest,
    ) -> Result<Value, InterceptError> {
        let body = req.read_json().await?;
        let parsed: CollabExecuteRequest = parse_body(body)?;

        let scope_id = parsed
            .plan
            .scope_id
            .clone()
            .unwrap_or_else(|| format!("collab-{}", tenant_id));

        let payload = json!({
            "scope_id": scope_id,
            "participants": parsed.plan.participants.clone().unwrap_or_default(),
            "summary_ref": Value::Null,
            "assigned_to": Value::Null,
            "degradation_reason": Value::Null,
            "attributes": parsed.plan.scope.clone(),
            "metadata": json!({
                "order": parsed.plan.order,
                "rounds": parsed.plan.rounds,
                "privacy_mode": parsed.plan.privacy_mode,
                "stub": true
            }),
            "manifest": json!({
                "provider": "sb-gateway",
                "scope_hint": parsed.plan.scope,
            }),
            "awareness": [
                json!({
                    "event_type": "collab_requested",
                    "payload": { "stub": true },
                    "degradation_reason": Value::Null,
                    "barrier_id": parsed.plan.barrier.mode
                }),
                json!({
                    "event_type": "collab_resolved",
                    "payload": { "stub": true },
                    "degradation_reason": Value::Null,
                    "barrier_id": parsed.plan.barrier.mode
                })
            ]
        });

        cx.response_status = Some(200);
        let scope_dbg = payload
            .get("scope_id")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        debug!("collab execute tenant={} scope={}", tenant_id, scope_dbg);
        Ok(success_envelope(payload, &cx.request_id))
    }
}

fn success_envelope(data: Value, trace: &str) -> Value {
    json!({
        "success": true,
        "data": data,
        "error": Value::Null,
        "trace_id": trace
    })
}

fn parse_body<T: DeserializeOwned>(value: Value) -> Result<T, InterceptError> {
    serde_json::from_value(value).map_err(|err| schema_error(format!("请求体解析失败: {err}")))
}

fn schema_error(msg: impl Into<String>) -> InterceptError {
    InterceptError::from_public(codes::SCHEMA_VALIDATION_FAILED, msg)
}

#[derive(Debug, Deserialize)]
struct ToolExecuteRequest {
    #[allow(dead_code)]
    anchor: Option<Value>,
    plan: ToolPlan,
    #[serde(default)]
    cycle: Value,
    #[serde(default)]
    router: Value,
    #[serde(default)]
    budget: Value,
}

#[derive(Debug, Deserialize)]
struct ToolPlan {
    nodes: Vec<ToolPlanNode>,
    #[serde(default)]
    edges: Vec<Value>,
    #[serde(default)]
    barrier: ToolPlanBarrier,
}

#[derive(Debug, Deserialize)]
struct ToolPlanNode {
    id: String,
    tool_id: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    input: Value,
    #[serde(default)]
    timeout_ms: Option<u32>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct ToolPlanBarrier {
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct CollabExecuteRequest {
    #[allow(dead_code)]
    anchor: Option<Value>,
    plan: CollabPlan,
}

#[derive(Debug, Deserialize)]
struct CollabPlan {
    #[serde(default)]
    scope_id: Option<String>,
    #[serde(default)]
    scope: Value,
    #[serde(default)]
    order: Option<String>,
    #[serde(default)]
    rounds: Option<u32>,
    #[serde(default)]
    privacy_mode: Option<String>,
    #[serde(default)]
    barrier: ToolPlanBarrier,
    #[serde(default)]
    participants: Option<Vec<Value>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use sb_interceptors::context::ProtoRequest;

    struct TestRequest {
        body: Value,
    }

    impl TestRequest {
        fn new(body: Value) -> Self {
            Self { body }
        }
    }

    #[async_trait]
    impl ProtoRequest for TestRequest {
        fn method(&self) -> &str {
            "POST"
        }

        fn path(&self) -> &str {
            "/tenants/1/tools.execute"
        }

        fn header(&self, _name: &str) -> Option<String> {
            None
        }

        async fn read_json(&mut self) -> Result<Value, InterceptError> {
            Ok(self.body.clone())
        }
    }

    #[tokio::test]
    async fn tool_execute_stub_returns_success() {
        let service = Arc::new(GatewayService::new(GatewayConfig {
            bind_addr: "0.0.0.0:0".into(),
        }));
        let mut ctx = InterceptContext::new();
        let body = json!({
            "plan": {
                "nodes": [
                    {"id": "n1", "tool_id": "demo", "input": {"demo": true}}
                ],
                "barrier": {"mode": "all"}
            }
        });
        let mut req = TestRequest::new(body);
        let response = service
            .clone()
            .handle_tools_execute(1, &mut ctx, &mut req)
            .await
            .expect("tool execute response");
        assert_eq!(ctx.response_status, Some(200));
        assert!(response["success"].as_bool().unwrap_or(false));
    }
}

