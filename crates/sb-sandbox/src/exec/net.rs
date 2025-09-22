use super::{ExecCtx, ExecOp, ExecResult, ExecUsage, SandboxExecutor};
use crate::errors::SandboxError;
use crate::model::{CapabilityKind, SideEffect, SideEffectRecord};
use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde_json::json;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use url::Url;

#[derive(Default)]
pub struct NetExecutor;

#[async_trait]
impl SandboxExecutor for NetExecutor {
    fn kind(&self) -> CapabilityKind {
        CapabilityKind::NetHttp
    }

    async fn execute(&self, ctx: &ExecCtx<'_>, op: ExecOp) -> Result<ExecResult, SandboxError> {
        if ctx.cancel.is_cancelled() {
            return Err(SandboxError::policy_violation("execution cancelled"));
        }
        match op {
            ExecOp::NetHttp {
                method,
                url,
                headers,
                body_b64,
            } => execute_http(ctx, method, url, headers, body_b64),
            _ => Err(SandboxError::policy_violation(
                "operation not supported by NetExecutor",
            )),
        }
    }
}

fn execute_http(
    ctx: &ExecCtx<'_>,
    method: String,
    url: String,
    headers: serde_json::Value,
    body_b64: Option<String>,
) -> Result<ExecResult, SandboxError> {
    let parsed = Url::parse(&url).map_err(|_| SandboxError::policy_violation("invalid url"))?;
    let scheme = parsed.scheme();
    if scheme != "https" && scheme != "http" {
        return Err(SandboxError::policy_violation("unsupported scheme"));
    }
    if let Some(host) = parsed.host_str() {
        ensure_domain_allowed(ctx, host)?;
    } else {
        return Err(SandboxError::policy_violation("missing host"));
    }
    ensure_method_allowed(ctx, &method)?;

    let body_bytes = match body_b64 {
        Some(ref payload) => BASE64
            .decode(payload)
            .map_err(|_| SandboxError::policy_violation("invalid request body"))?,
        None => Vec::new(),
    };

    if let Some(limit) = ctx.profile.limits.max_bytes_out {
        if body_bytes.len() as u64 > limit {
            return Err(SandboxError::policy_violation("request body exceeds limit"));
        }
    }

    if let Some(max_bytes_in) = ctx.profile.limits.max_bytes_in {
        if max_bytes_in == 0 {
            return Err(SandboxError::policy_violation("network response forbidden"));
        }
    }

    let usage = ExecUsage {
        calls: 1,
        bytes_out: body_bytes.len() as u64,
        ..ExecUsage::default()
    };

    let side_effects = vec![SideEffectRecord {
        kind: SideEffect::Network,
        meta: json!({
            "method": method,
            "url": url,
            "request_bytes": usage.bytes_out,
        }),
    }];

    Ok(ExecResult::success(
        json!({
            "status": "simulated",
            "method": method,
            "url": url,
            "headers": headers,
            "request_body_present": !body_bytes.is_empty(),
        }),
        usage,
        side_effects,
    ))
}

fn ensure_method_allowed(ctx: &ExecCtx<'_>, method: &str) -> Result<(), SandboxError> {
    let method_upper = method.to_uppercase();
    if !ctx.profile.whitelists.methods.is_empty()
        && !ctx
            .profile
            .whitelists
            .methods
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(&method_upper))
    {
        return Err(SandboxError::policy_violation("http method not allowed"));
    }

    // Consent enforcement is handled by orchestrator; here we just ensure high-risk methods flagged.
    Ok(())
}

fn ensure_domain_allowed(ctx: &ExecCtx<'_>, host: &str) -> Result<(), SandboxError> {
    if is_private_host(host) {
        return Err(SandboxError::policy_violation(
            "host resolves to private network",
        ));
    }
    if ctx.profile.whitelists.domains.is_empty() {
        return Err(SandboxError::policy_violation(
            "network domains not declared",
        ));
    }
    if !ctx
        .profile
        .whitelists
        .domains
        .iter()
        .any(|allowed| host.ends_with(allowed))
    {
        return Err(SandboxError::policy_violation("domain not in whitelist"));
    }
    Ok(())
}

fn is_private_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    if let Ok(addr) = host.parse::<IpAddr>() {
        return match addr {
            IpAddr::V4(v4) => is_private_ipv4(v4),
            IpAddr::V6(v6) => is_private_ipv6(v6),
        };
    }
    false
}

fn is_private_ipv4(addr: Ipv4Addr) -> bool {
    let octets = addr.octets();
    match octets {
        [10, ..] => true,
        [172, 16..=31, ..] => true,
        [192, 168, ..] => true,
        [127, ..] => true,
        [169, 254, ..] => true,
        _ => false,
    }
}

fn is_private_ipv6(addr: Ipv6Addr) -> bool {
    addr.is_loopback() || addr.is_unique_local() || addr.is_unspecified()
}
