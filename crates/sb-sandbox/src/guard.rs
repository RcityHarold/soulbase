use crate::errors::SandboxError;
use crate::model::{Capability, Profile};
use async_trait::async_trait;
use std::path::{Component, Path};

#[async_trait]
pub trait PolicyGuard: Send + Sync {
    async fn validate(
        &self,
        profile: &Profile,
        capability: &Capability,
    ) -> Result<(), SandboxError>;
}

#[derive(Default)]
pub struct DefaultPolicyGuard;

#[async_trait]
impl PolicyGuard for DefaultPolicyGuard {
    async fn validate(
        &self,
        profile: &Profile,
        capability: &Capability,
    ) -> Result<(), SandboxError> {
        if !profile.capabilities.contains(capability) {
            return Err(SandboxError::permission_denied(
                "capability not allowed in profile",
            ));
        }
        match capability {
            Capability::FsRead { path }
            | Capability::FsWrite { path, .. }
            | Capability::FsList { path } => validate_path(path, profile)?,
            Capability::NetHttp { host, .. } => validate_domain(host, profile)?,
            Capability::ProcExec { tool } => validate_tool(tool, profile)?,
            Capability::BrowserUse { .. } | Capability::TmpUse | Capability::SysGpu { .. } => {}
        }
        Ok(())
    }
}

fn validate_path(path: &str, profile: &Profile) -> Result<(), SandboxError> {
    let normalized = normalize_path(path);
    if let Some(root) = profile.mappings.root_fs.as_ref() {
        let target = normalize_path(root);
        if !normalized.starts_with(&target) {
            return Err(SandboxError::policy_violation(
                "path outside of mapped root",
            ));
        }
    }
    if !profile.whitelists.paths.is_empty()
        && !profile
            .whitelists
            .paths
            .iter()
            .any(|allowed| normalized.starts_with(allowed))
    {
        return Err(SandboxError::policy_violation("path not in whitelist"));
    }
    Ok(())
}

fn validate_domain(domain: &str, profile: &Profile) -> Result<(), SandboxError> {
    if profile.whitelists.domains.is_empty() {
        return Err(SandboxError::policy_violation(
            "network domains not declared",
        ));
    }
    if !profile
        .whitelists
        .domains
        .iter()
        .any(|allowed| domain.ends_with(allowed))
    {
        return Err(SandboxError::policy_violation("domain not allowed"));
    }
    Ok(())
}

fn validate_tool(tool: &str, profile: &Profile) -> Result<(), SandboxError> {
    if profile.whitelists.tools.is_empty() {
        return Err(SandboxError::policy_violation(
            "no allowed tools configured",
        ));
    }
    if !profile
        .whitelists
        .tools
        .iter()
        .any(|allowed| allowed == tool)
    {
        return Err(SandboxError::policy_violation("tool not allowed"));
    }
    Ok(())
}

pub fn normalize_path(path: &str) -> String {
    let p = Path::new(path);
    let mut segments: Vec<String> = Vec::new();
    for comp in p.components() {
        match comp {
            Component::RootDir => {
                segments.clear();
                segments.push(String::new());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                segments.pop();
            }
            Component::Normal(part) => {
                segments.push(part.to_string_lossy().into_owned());
            }
            Component::Prefix(prefix) => {
                segments.push(prefix.as_os_str().to_string_lossy().into_owned());
            }
        }
    }
    let joined = if segments.is_empty() {
        "/".to_string()
    } else {
        let value = segments.join("/");
        if value.is_empty() {
            "/".to_string()
        } else if value.starts_with('/') {
            value
        } else {
            format!("/{}", value)
        }
    };
    joined
}
