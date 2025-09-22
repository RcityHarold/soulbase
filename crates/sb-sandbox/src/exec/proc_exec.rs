use super::{ExecCtx, ExecOp, ExecResult, ExecUsage, SandboxExecutor};
use crate::errors::SandboxError;
use crate::model::{CapabilityKind, SideEffect, SideEffectRecord};
use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde_json::json;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct ProcessExecutor;

#[async_trait]
impl SandboxExecutor for ProcessExecutor {
    fn kind(&self) -> CapabilityKind {
        CapabilityKind::ProcExec
    }

    async fn execute(&self, ctx: &ExecCtx<'_>, op: ExecOp) -> Result<ExecResult, SandboxError> {
        if ctx.cancel.is_cancelled() {
            return Err(SandboxError::policy_violation("execution cancelled"));
        }
        match op {
            ExecOp::ProcExec {
                tool,
                args,
                timeout_ms,
            } => {
                ensure_tool_allowed(ctx, &tool)?;
                ensure_args_safe(&args)?;
                let mut command = Command::new(&tool);
                command.args(&args);
                command.stdin(Stdio::null());
                command.stderr(Stdio::piped());
                command.stdout(Stdio::piped());
                command.env_clear();
                if let Some(root) = ctx.profile.mappings.tmp_dir.as_ref() {
                    command.current_dir(root);
                }

                let timeout = timeout_ms
                    .or(Some(ctx.profile.timeout_ms))
                    .unwrap_or(30_000);

                let start = Instant::now();
                let output = command
                    .output()
                    .map_err(|_| SandboxError::policy_violation("failed to spawn process"))?;
                if start.elapsed() > Duration::from_millis(timeout as u64) {
                    return Err(SandboxError::policy_violation("process timeout exceeded"));
                }

                let stdout_len = output.stdout.len() as u64;
                let stderr_len = output.stderr.len() as u64;
                if let Some(limit) = ctx.profile.limits.max_bytes_in {
                    if stdout_len + stderr_len > limit {
                        return Err(SandboxError::policy_violation(
                            "process output exceeds byte limit",
                        ));
                    }
                }

                let usage = ExecUsage {
                    calls: 1,
                    bytes_in: stdout_len + stderr_len,
                    ..ExecUsage::default()
                };
                let side_effects = vec![SideEffectRecord {
                    kind: SideEffect::Process,
                    meta: json!({
                        "tool": tool,
                        "args": args,
                        "status": output.status.code(),
                        "stdout_bytes": stdout_len,
                        "stderr_bytes": stderr_len,
                    }),
                }];
                Ok(ExecResult::success(
                    json!({
                        "status": output.status.code(),
                        "stdout_b64": BASE64.encode(output.stdout),
                        "stderr_b64": BASE64.encode(output.stderr),
                    }),
                    usage,
                    side_effects,
                ))
            }
            _ => Err(SandboxError::policy_violation(
                "operation not supported by ProcessExecutor",
            )),
        }
    }
}

fn ensure_tool_allowed(ctx: &ExecCtx<'_>, tool: &str) -> Result<(), SandboxError> {
    if ctx.profile.whitelists.tools.is_empty() {
        return Err(SandboxError::policy_violation(
            "process execution disabled by policy",
        ));
    }
    if !ctx
        .profile
        .whitelists
        .tools
        .iter()
        .any(|allowed| allowed == tool)
    {
        return Err(SandboxError::policy_violation("tool not allowed"));
    }
    Ok(())
}

fn ensure_args_safe(args: &[String]) -> Result<(), SandboxError> {
    for arg in args {
        if arg.contains(';') || arg.contains('|') || arg.contains('&') || arg.contains('`') {
            return Err(SandboxError::policy_violation(
                "argument contains unsafe characters",
            ));
        }
    }
    Ok(())
}
