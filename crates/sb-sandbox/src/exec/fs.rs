use super::{ExecCtx, ExecOp, ExecResult, ExecUsage, SandboxExecutor};
use crate::errors::SandboxError;
use crate::model::{CapabilityKind, SideEffect, SideEffectRecord};
use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde_json::json;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;

#[derive(Default)]
pub struct FsExecutor;

#[async_trait]
impl SandboxExecutor for FsExecutor {
    fn kind(&self) -> CapabilityKind {
        CapabilityKind::FsRead
    }

    async fn execute(&self, ctx: &ExecCtx<'_>, op: ExecOp) -> Result<ExecResult, SandboxError> {
        if ctx.cancel.is_cancelled() {
            return Err(SandboxError::policy_violation("execution cancelled"));
        }
        match op {
            ExecOp::FsRead { path, offset, len } => read_file(ctx, &path, offset, len),
            ExecOp::FsWrite {
                path,
                bytes_b64,
                overwrite,
            } => write_file(ctx, &path, &bytes_b64, overwrite),
            ExecOp::FsList { path } => list_dir(ctx, &path),
            _ => Err(SandboxError::policy_violation(
                "operation not supported by FsExecutor",
            )),
        }
    }
}

fn read_file(
    ctx: &ExecCtx<'_>,
    path: &str,
    offset: Option<u64>,
    len: Option<u64>,
) -> Result<ExecResult, SandboxError> {
    let mut file = fs::File::open(path)
        .map_err(|_| SandboxError::policy_violation("failed to open file for read"))?;
    if let Some(off) = offset {
        use std::io::Seek;
        file.seek(std::io::SeekFrom::Start(off))
            .map_err(|_| SandboxError::policy_violation("failed to seek file"))?;
    }
    let mut buffer = Vec::new();
    let allowed = ctx.profile.limits.max_bytes_in.unwrap_or(u64::MAX);
    let to_read = len.unwrap_or(allowed).min(allowed);
    if to_read < u64::MAX {
        let mut limited = file.take(to_read);
        limited
            .read_to_end(&mut buffer)
            .map_err(|_| SandboxError::policy_violation("failed to read file"))?;
    } else {
        file.read_to_end(&mut buffer)
            .map_err(|_| SandboxError::policy_violation("failed to read file"))?;
    }
    let encoded = BASE64.encode(&buffer);
    let usage = ExecUsage {
        bytes_in: buffer.len() as u64,
        calls: 1,
        ..ExecUsage::default()
    };
    if let Some(max_in) = ctx.profile.limits.max_bytes_in {
        if usage.bytes_in > max_in {
            return Err(SandboxError::policy_violation("read exceeds byte limit"));
        }
    }
    let side_effects = vec![SideEffectRecord {
        kind: SideEffect::Read,
        meta: json!({
            "path": path,
            "bytes": usage.bytes_in,
        }),
    }];
    Ok(ExecResult::success(
        json!({ "data_b64": encoded }),
        usage,
        side_effects,
    ))
}

fn write_file(
    ctx: &ExecCtx<'_>,
    path: &str,
    data_b64: &str,
    overwrite: bool,
) -> Result<ExecResult, SandboxError> {
    let bytes = BASE64
        .decode(data_b64)
        .map_err(|_| SandboxError::policy_violation("invalid base64 payload"))?;
    if let Some(limit) = ctx.profile.limits.max_bytes_out {
        if bytes.len() as u64 > limit {
            return Err(SandboxError::policy_violation("write exceeds byte limit"));
        }
    }
    if let Some(max_files) = ctx.profile.limits.max_files {
        if max_files == 0 {
            return Err(SandboxError::policy_violation(
                "file writes disabled by policy",
            ));
        }
    }
    if Path::new(path).exists() && !overwrite {
        return Err(SandboxError::policy_violation(
            "file exists and overwrite disabled",
        ));
    }
    if let Some(parent) = Path::new(path).parent() {
        fs::create_dir_all(parent)
            .map_err(|_| SandboxError::policy_violation("failed to create directories"))?;
    }
    let mut file = fs::File::create(path)
        .map_err(|_| SandboxError::policy_violation("failed to open file for write"))?;
    file.write_all(&bytes)
        .map_err(|_| SandboxError::policy_violation("failed to write file"))?;
    let usage = ExecUsage {
        bytes_out: bytes.len() as u64,
        calls: 1,
        file_count: 1,
        ..ExecUsage::default()
    };
    let side_effects = vec![SideEffectRecord {
        kind: SideEffect::Write,
        meta: json!({
            "path": path,
            "bytes": usage.bytes_out,
        }),
    }];
    Ok(ExecResult::success(
        json!({ "written_bytes": bytes.len() }),
        usage,
        side_effects,
    ))
}

fn list_dir(ctx: &ExecCtx<'_>, path: &str) -> Result<ExecResult, SandboxError> {
    let entries = fs::read_dir(path)
        .map_err(|_| SandboxError::policy_violation("failed to list directory"))?;
    let mut items = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_| SandboxError::policy_violation("failed to read entry"))?;
        let file_type = entry
            .file_type()
            .map_err(|_| SandboxError::policy_violation("failed to inspect entry"))?;
        let kind = if file_type.is_dir() {
            "dir"
        } else if file_type.is_file() {
            "file"
        } else {
            "other"
        };
        items.push(json!({
            "name": entry.file_name(),
            "kind": kind,
        }));
    }
    if let Some(max_files) = ctx.profile.limits.max_files {
        if items.len() as u64 > max_files {
            return Err(SandboxError::policy_violation(
                "directory listing exceeds limit",
            ));
        }
    }
    let usage = ExecUsage {
        calls: 1,
        file_count: items.len() as u64,
        ..ExecUsage::default()
    };
    let side_effects = vec![SideEffectRecord {
        kind: SideEffect::Filesystem,
        meta: json!({
            "path": path,
            "count": items.len(),
        }),
    }];
    Ok(ExecResult::success(
        json!({ "entries": items }),
        usage,
        side_effects,
    ))
}
