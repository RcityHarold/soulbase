use crate::config::PolicyConfig;
use crate::errors::SandboxError;
use crate::model::{
    Budget, Capability, Grant, Limits, Mappings, Profile, SafetyClass, SideEffect, ToolManifest,
    Whitelists,
};
use async_trait::async_trait;
use hex;
use serde_json;
use sha2::{Digest, Sha256};
use std::collections::HashSet;

#[async_trait]
pub trait ProfileBuilder: Send + Sync {
    async fn build(
        &self,
        grant: &Grant,
        manifest: &ToolManifest,
        policy: &PolicyConfig,
    ) -> Result<Profile, SandboxError>;
}

#[derive(Default)]
pub struct DefaultProfileBuilder;

#[async_trait]
impl ProfileBuilder for DefaultProfileBuilder {
    async fn build(
        &self,
        grant: &Grant,
        manifest: &ToolManifest,
        policy: &PolicyConfig,
    ) -> Result<Profile, SandboxError> {
        let caps = intersect_capabilities(
            &grant.capabilities,
            &manifest.capabilities,
            &policy.capabilities,
        );
        if caps.is_empty() {
            return Err(SandboxError::forbidden(
                "capability set is empty after intersection",
            ));
        }

        let safety = max_safety(manifest.safety, policy.safety_class);
        let side_effects = union_side_effects(&manifest.side_effects, &policy.side_effects);
        let limits = merge_limits(
            manifest.limits.as_ref(),
            policy.limits.as_ref(),
            Some(&grant.budget),
        );
        let whitelists = merge_whitelists(manifest.whitelists.as_ref(), policy.whitelists.as_ref());
        let mappings = merge_mappings(manifest.mappings.as_ref(), policy.mappings.as_ref());
        let timeout_ms = min_timeout(
            manifest.timeout_ms,
            policy.timeout_ms,
            policy.defaults.timeout_ms,
        );
        let policy_hash = compute_policy_hash(policy);
        let config_version = policy.config_version.clone();
        let config_hash = policy.config_hash.clone().or_else(|| policy_hash.clone());

        let mut profile = Profile {
            tenant: grant.tenant.clone(),
            subject_id: grant.subject_id.clone(),
            tool_name: manifest.name.clone(),
            call_id: grant.call_id.clone(),
            capabilities: caps,
            safety,
            side_effects,
            limits,
            whitelists,
            mappings,
            timeout_ms,
            profile_hash: String::new(),
            policy_hash,
            config_version,
            config_hash,
        };
        profile.profile_hash = profile.hash();
        Ok(profile)
    }
}

fn intersect_capabilities(
    grant: &[Capability],
    manifest: &[Capability],
    policy: &[Capability],
) -> Vec<Capability> {
    let manifest_set: HashSet<_> = manifest.iter().cloned().collect();
    let policy_set: HashSet<_> = if policy.is_empty() {
        manifest_set.clone()
    } else {
        policy.iter().cloned().collect()
    };
    grant
        .iter()
        .filter(|cap| manifest_set.contains(*cap) && policy_set.contains(*cap))
        .cloned()
        .collect()
}

fn max_safety(a: SafetyClass, b: SafetyClass) -> SafetyClass {
    std::cmp::max(a, b)
}

fn union_side_effects(a: &[SideEffect], b: &[SideEffect]) -> Vec<SideEffect> {
    let mut set: HashSet<SideEffect> = a.iter().cloned().collect();
    set.extend(b.iter().cloned());
    let mut list: Vec<_> = set.into_iter().collect();
    list.sort();
    list
}

fn merge_limits(
    manifest: Option<&Limits>,
    policy: Option<&Limits>,
    budget: Option<&Budget>,
) -> Limits {
    fn min_opt(a: Option<u64>, b: Option<u64>) -> Option<u64> {
        match (a, b) {
            (Some(x), Some(y)) => Some(x.min(y)),
            (Some(x), None) => Some(x),
            (None, Some(y)) => Some(y),
            (None, None) => None,
        }
    }

    fn min_opt_u32(a: Option<u32>, b: Option<u32>) -> Option<u32> {
        match (a, b) {
            (Some(x), Some(y)) => Some(x.min(y)),
            (Some(x), None) => Some(x),
            (None, Some(y)) => Some(y),
            (None, None) => None,
        }
    }

    let mut limits = Limits::default();
    let mut max_bytes_in = min_opt(
        manifest.and_then(|m| m.max_bytes_in),
        policy.and_then(|p| p.max_bytes_in),
    );
    let mut max_bytes_out = min_opt(
        manifest.and_then(|m| m.max_bytes_out),
        policy.and_then(|p| p.max_bytes_out),
    );
    let mut max_files = min_opt(
        manifest.and_then(|m| m.max_files),
        policy.and_then(|p| p.max_files),
    );

    if let Some(budget) = budget {
        if budget.bytes_in > 0 {
            max_bytes_in = Some(max_bytes_in.unwrap_or(budget.bytes_in).min(budget.bytes_in));
        }
        if budget.bytes_out > 0 {
            max_bytes_out = Some(
                max_bytes_out
                    .unwrap_or(budget.bytes_out)
                    .min(budget.bytes_out),
            );
        }
        if budget.file_count > 0 {
            max_files = Some(
                max_files
                    .unwrap_or(budget.file_count)
                    .min(budget.file_count),
            );
        }
    }

    limits.max_bytes_in = max_bytes_in;
    limits.max_bytes_out = max_bytes_out;
    limits.max_files = max_files;
    limits.max_depth = min_opt_u32(
        manifest.and_then(|m| m.max_depth),
        policy.and_then(|p| p.max_depth),
    );
    limits.max_concurrency = min_opt_u32(
        manifest.and_then(|m| m.max_concurrency),
        policy.and_then(|p| p.max_concurrency),
    );
    limits
}

fn merge_whitelists(manifest: Option<&Whitelists>, policy: Option<&Whitelists>) -> Whitelists {
    fn intersect_list(a: &[String], b: &[String]) -> Vec<String> {
        if a.is_empty() {
            return b.to_vec();
        }
        if b.is_empty() {
            return a.to_vec();
        }
        let set_b: HashSet<&String> = b.iter().collect();
        a.iter()
            .filter(|item| set_b.contains(item))
            .cloned()
            .collect()
    }

    match (manifest, policy) {
        (Some(m), Some(p)) => Whitelists {
            domains: intersect_list(&m.domains, &p.domains),
            paths: intersect_list(&m.paths, &p.paths),
            tools: intersect_list(&m.tools, &p.tools),
            mime_allow: intersect_list(&m.mime_allow, &p.mime_allow),
            methods: intersect_list(&m.methods, &p.methods),
        },
        (Some(m), None) => m.clone(),
        (None, Some(p)) => p.clone(),
        (None, None) => Whitelists::default(),
    }
}

fn merge_mappings(manifest: Option<&Mappings>, policy: Option<&Mappings>) -> Mappings {
    let mut mappings = Mappings::default();
    mappings.root_fs = policy
        .and_then(|p| p.root_fs.clone())
        .or_else(|| manifest.and_then(|m| m.root_fs.clone()));
    mappings.tmp_dir = policy
        .and_then(|p| p.tmp_dir.clone())
        .or_else(|| manifest.and_then(|m| m.tmp_dir.clone()));
    mappings
}

fn min_timeout(manifest: Option<u64>, policy: Option<u64>, default_timeout: Option<u64>) -> u64 {
    let mut timeouts: Vec<u64> = vec![];
    if let Some(m) = manifest {
        timeouts.push(m);
    }
    if let Some(p) = policy {
        timeouts.push(p);
    }
    if let Some(d) = default_timeout {
        timeouts.push(d);
    }
    if timeouts.is_empty() {
        30_000 // 默认 30s
    } else {
        *timeouts.iter().min().unwrap_or(&30_000)
    }
}

fn compute_policy_hash(policy: &PolicyConfig) -> Option<String> {
    if let Some(hash) = policy.policy_hash.clone() {
        return Some(hash);
    }
    serde_json::to_vec(policy).ok().map(|bytes| {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        hex::encode(hasher.finalize())
    })
}
