use crate::context::{InterceptContext, ProtoResponse};
use crate::errors::InterceptError;
use crate::stages::ResponseStage;
use sb_auth::prelude::Obligation;
use sb_errors::prelude::codes;

pub struct ObligationsStage;

impl ResponseStage for ObligationsStage {
    fn handle_response(
        &self,
        cx: &mut InterceptContext,
        _rsp: &mut dyn ProtoResponse,
    ) -> Result<(), InterceptError> {
        let obligations = cx.obligations.clone();
        for obligation in obligations {
            let body = cx.ensure_response_body();
            apply_obligation(body, &obligation)?;
        }
        Ok(())
    }
}

fn apply_obligation(
    body: &mut serde_json::Value,
    obligation: &Obligation,
) -> Result<(), InterceptError> {
    match obligation.kind.as_str() {
        "mask" => {
            let path = path_segments(obligation, "path")?;
            let replacement = obligation
                .params
                .get("replacement")
                .and_then(|v| v.as_str())
                .unwrap_or("****");
            let target = descend_mut(body, &path).ok_or_else(|| {
                InterceptError::from_public(codes::POLICY_DENY_TOOL, "无法执行脱敏义务。")
            })?;
            *target = serde_json::Value::String(replacement.to_string());
            Ok(())
        }
        "redact" => {
            let path = path_segments(obligation, "path")?;
            if !remove_path(body, &path) {
                return Err(InterceptError::from_public(
                    codes::POLICY_DENY_TOOL,
                    "无法执行删除义务。",
                ));
            }
            Ok(())
        }
        "watermark" => {
            let mark = obligation
                .params
                .get("value")
                .and_then(|v| v.as_str())
                .unwrap_or("watermark");
            body.as_object_mut()
                .ok_or_else(|| {
                    InterceptError::from_public(
                        codes::POLICY_DENY_TOOL,
                        "响应不是 JSON 对象，无法打水印。",
                    )
                })?
                .insert(
                    "__watermark".to_string(),
                    serde_json::Value::String(mark.to_string()),
                );
            Ok(())
        }
        _ => Ok(()),
    }
}

fn path_segments<'a>(obligation: &'a Obligation, key: &str) -> Result<Vec<String>, InterceptError> {
    obligation
        .params
        .get(key)
        .and_then(|v| v.as_str())
        .map(|raw| raw.split('.').map(|s| s.to_string()).collect())
        .ok_or_else(|| InterceptError::from_public(codes::POLICY_DENY_TOOL, "义务缺少路径。"))
}

fn descend_mut<'a>(
    value: &'a mut serde_json::Value,
    path: &[String],
) -> Option<&'a mut serde_json::Value> {
    if path.is_empty() {
        return Some(value);
    }
    match value {
        serde_json::Value::Object(map) => {
            let key = &path[0];
            let next = map.get_mut(key)?;
            descend_mut(next, &path[1..])
        }
        _ => None,
    }
}

fn remove_path(value: &mut serde_json::Value, path: &[String]) -> bool {
    if path.is_empty() {
        return false;
    }
    if path.len() == 1 {
        if let serde_json::Value::Object(map) = value {
            return map.remove(&path[0]).is_some();
        }
        return false;
    }
    if let serde_json::Value::Object(map) = value {
        if let Some(next) = map.get_mut(&path[0]) {
            return remove_path(next, &path[1..]);
        }
    }
    false
}
