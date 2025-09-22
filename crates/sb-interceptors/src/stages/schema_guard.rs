use crate::context::{InterceptContext, ProtoRequest, ProtoResponse};
use crate::errors::InterceptError;
use crate::schema::JsonSchemaRegistry;
use crate::stages::{ResponseStage, Stage, StageOutcome};
use async_trait::async_trait;
use sb_errors::prelude::{codes, ErrorBuilder};
use std::sync::Arc;

#[derive(Clone)]
pub struct SchemaGuardStage {
    registry: Option<Arc<JsonSchemaRegistry>>,
}

impl SchemaGuardStage {
    pub fn new(registry: Option<Arc<JsonSchemaRegistry>>) -> Self {
        Self { registry }
    }

    fn guard(&self, schema: &str, payload: &serde_json::Value) -> Result<(), InterceptError> {
        let Some(registry) = &self.registry else {
            return Ok(());
        };
        if let Err(errors) = registry.validate(schema, payload) {
            let detail = if errors.is_empty() {
                "Schema validation failed".to_string()
            } else {
                errors.join("; ")
            };
            return Err(InterceptError::new(
                ErrorBuilder::new(codes::SCHEMA_VALIDATION_FAILED)
                    .user_msg("请求数据格式不符合要求。")
                    .dev_msg(detail)
                    .build(),
            ));
        }
        Ok(())
    }
}

impl Default for SchemaGuardStage {
    fn default() -> Self {
        Self { registry: None }
    }
}

#[async_trait]
impl Stage for SchemaGuardStage {
    async fn handle(
        &self,
        cx: &mut InterceptContext,
        req: &mut dyn ProtoRequest,
        _rsp: &mut dyn ProtoResponse,
    ) -> Result<StageOutcome, InterceptError> {
        let Some(route) = cx.route.as_ref() else {
            return Ok(StageOutcome::Continue);
        };
        let Some(schema_name) = route.request_schema.as_deref() else {
            return Ok(StageOutcome::Continue);
        };
        let payload = req.read_json().await?;
        self.guard(schema_name, &payload)?;
        cx.extensions
            .insert("last_request_body".to_string(), payload);
        Ok(StageOutcome::Continue)
    }
}

impl ResponseStage for SchemaGuardStage {
    fn handle_response(
        &self,
        cx: &mut InterceptContext,
        _rsp: &mut dyn ProtoResponse,
    ) -> Result<(), InterceptError> {
        let Some(route) = cx.route.as_ref() else {
            return Ok(());
        };
        let Some(schema_name) = route.response_schema.as_deref() else {
            return Ok(());
        };
        if let Some(body) = cx.response_body.as_ref() {
            self.guard(schema_name, body)?;
        }
        Ok(())
    }
}
