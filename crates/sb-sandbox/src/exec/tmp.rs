use super::{ExecCtx, ExecOp, ExecResult, ExecUsage, SandboxExecutor};
use crate::errors::SandboxError;
use crate::model::{CapabilityKind, SideEffect, SideEffectRecord};
use async_trait::async_trait;
use serde_json::json;
use std::fs;

#[derive(Default)]
pub struct TmpExecutor;

#[async_trait]
impl SandboxExecutor for TmpExecutor {
    fn kind(&self) -> CapabilityKind {
        CapabilityKind::TmpUse
    }

    async fn execute(&self, ctx: &ExecCtx<'_>, op: ExecOp) -> Result<ExecResult, SandboxError> {
        if ctx.cancel.is_cancelled() {
            return Err(SandboxError::policy_violation("execution cancelled"));
        }
        match op {
            ExecOp::TmpAlloc { size_bytes } => {
                if let Some(limit) = ctx.profile.limits.max_bytes_out {
                    if size_bytes > limit {
                        return Err(SandboxError::policy_violation(
                            "tmp allocation exceeds limit",
                        ));
                    }
                }
                if let Some(tmp) = &ctx.profile.mappings.tmp_dir {
                    fs::create_dir_all(tmp)
                        .map_err(|_| SandboxError::policy_violation("failed to create tmp dir"))?;
                }
                let side_effects = vec![SideEffectRecord {
                    kind: SideEffect::Filesystem,
                    meta: json!({
                        "tmp_dir": ctx.profile.mappings.tmp_dir,
                        "bytes": size_bytes,
                    }),
                }];
                Ok(ExecResult::success(
                    json!({ "allocated": size_bytes }),
                    ExecUsage {
                        bytes_out: size_bytes,
                        calls: 1,
                        ..ExecUsage::default()
                    },
                    side_effects,
                ))
            }
            _ => Err(SandboxError::policy_violation(
                "operation not supported by TmpExecutor",
            )),
        }
    }
}
