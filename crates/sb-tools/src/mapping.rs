use crate::errors::{ToolError, ToolResult};
use crate::manifest::ToolManifest;
use sb_sandbox::prelude::{Capability, CapabilityKind, ExecOp};
use serde_json::Value;

pub fn manifest_to_capabilities(manifest: &ToolManifest) -> Vec<Capability> {
    manifest
        .capabilities
        .iter()
        .filter_map(|decl| match decl.domain.as_str() {
            "fs" => match decl.action.as_str() {
                "read" => Some(Capability::FsRead {
                    path: decl.resource.clone(),
                }),
                "write" => Some(Capability::FsWrite {
                    path: decl.resource.clone(),
                    append: false,
                }),
                "list" => Some(Capability::FsList {
                    path: decl.resource.clone(),
                }),
                _ => None,
            },
            "net.http" => Some(Capability::NetHttp {
                host: decl.resource.clone(),
                port: None,
                scheme: Some("https".into()),
                methods: vec![decl.action.to_uppercase()],
            }),
            "tmp" => Some(Capability::TmpUse),
            "browser" => Some(Capability::BrowserUse {
                scope: decl.resource.clone(),
            }),
            "proc" => Some(Capability::ProcExec {
                tool: decl.resource.clone(),
            }),
            _ => None,
        })
        .collect()
}

pub fn plan_exec_ops(manifest: &ToolManifest, args: &Value) -> ToolResult<Vec<ExecOp>> {
    let mut ops = Vec::new();
    for decl in &manifest.capabilities {
        match decl.domain.as_str() {
            "net.http" => ops.push(plan_http(&decl.action, args)?),
            "fs" => match decl.action.as_str() {
                "read" => ops.push(plan_fs_read(args)?),
                "write" => ops.push(plan_fs_write(args)?),
                _ => {}
            },
            "tmp" => ops.push(plan_tmp(args)?),
            _ => {}
        }
    }
    Ok(ops)
}

fn plan_http(method: &str, args: &Value) -> ToolResult<ExecOp> {
    let url = args
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::schema("missing field: url"))?;
    let headers = args
        .get("headers")
        .cloned()
        .unwrap_or_else(|| Value::Object(Default::default()));
    let body = args
        .get("body_b64")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    Ok(ExecOp::NetHttp {
        method: method.to_uppercase(),
        url: url.to_string(),
        headers,
        body_b64: body,
    })
}

fn plan_fs_read(args: &Value) -> ToolResult<ExecOp> {
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::schema("missing field: path"))?;
    let offset = args.get("offset").and_then(|v| v.as_u64());
    let len = args.get("len").and_then(|v| v.as_u64());
    Ok(ExecOp::FsRead {
        path: path.to_string(),
        offset,
        len,
    })
}

fn plan_fs_write(args: &Value) -> ToolResult<ExecOp> {
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::schema("missing field: path"))?;
    let bytes_b64 = args
        .get("content_b64")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::schema("missing field: content_b64"))?;
    Ok(ExecOp::FsWrite {
        path: path.to_string(),
        bytes_b64: bytes_b64.to_string(),
        overwrite: args
            .get("overwrite")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    })
}

fn plan_tmp(args: &Value) -> ToolResult<ExecOp> {
    let size = args
        .get("size_bytes")
        .and_then(|v| v.as_u64())
        .unwrap_or(1024);
    Ok(ExecOp::TmpAlloc { size_bytes: size })
}

pub fn infer_capability_kind(cap: &Capability) -> CapabilityKind {
    match cap {
        Capability::FsRead { .. } => CapabilityKind::FsRead,
        Capability::FsWrite { .. } => CapabilityKind::FsWrite,
        Capability::FsList { .. } => CapabilityKind::FsList,
        Capability::NetHttp { .. } => CapabilityKind::NetHttp,
        Capability::BrowserUse { .. } => CapabilityKind::BrowserUse,
        Capability::ProcExec { .. } => CapabilityKind::ProcExec,
        Capability::TmpUse => CapabilityKind::TmpUse,
        Capability::SysGpu { .. } => CapabilityKind::SysGpu,
    }
}
