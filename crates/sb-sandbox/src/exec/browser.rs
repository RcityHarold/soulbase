use super::{ExecCtx, ExecOp, ExecResult, ExecUsage, SandboxExecutor};
use crate::errors::SandboxError;
use crate::model::{CapabilityKind, SideEffect, SideEffectRecord};
use async_trait::async_trait;
use serde_json::json;

#[derive(Default)]
pub struct BrowserExecutor;

#[async_trait]
impl SandboxExecutor for BrowserExecutor {
    fn kind(&self) -> CapabilityKind {
        CapabilityKind::BrowserUse
    }

    async fn execute(&self, ctx: &ExecCtx<'_>, op: ExecOp) -> Result<ExecResult, SandboxError> {
        if ctx.cancel.is_cancelled() {
            return Err(SandboxError::policy_violation("execution cancelled"));
        }
        match op {
            ExecOp::BrowserNav { url } => {
                let usage = ExecUsage {
                    calls: 1,
                    ..ExecUsage::default()
                };
                let side_effects = vec![SideEffectRecord {
                    kind: SideEffect::Browser,
                    meta: json!({
                        "action": "navigate",
                        "url": url,
                    }),
                }];
                Ok(ExecResult::success(
                    json!({ "navigated_to": url }),
                    usage,
                    side_effects,
                ))
            }
            ExecOp::BrowserScreenshot {
                selector,
                full_page,
            } => {
                if let Some(limit) = ctx.profile.limits.max_bytes_in {
                    if limit == 0 {
                        return Err(SandboxError::policy_violation(
                            "browser screenshot disabled by policy",
                        ));
                    }
                }
                let usage = ExecUsage {
                    calls: 1,
                    ..ExecUsage::default()
                };
                let side_effects = vec![SideEffectRecord {
                    kind: SideEffect::Browser,
                    meta: json!({
                        "action": "screenshot",
                        "selector": selector,
                        "full_page": full_page,
                    }),
                }];
                Ok(ExecResult::success(
                    json!({
                        "screenshot": {
                            "selector": selector,
                            "full_page": full_page,
                        }
                    }),
                    usage,
                    side_effects,
                ))
            }
            _ => Err(SandboxError::policy_violation(
                "operation not supported by BrowserExecutor",
            )),
        }
    }
}
