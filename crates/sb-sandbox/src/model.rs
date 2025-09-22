use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{self, Value};
use sha2::{Digest, Sha256};

use sb_types::prelude::{Consent, Id, TenantId};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Capability {
    FsRead {
        path: String,
    },
    FsWrite {
        path: String,
        append: bool,
    },
    FsList {
        path: String,
    },
    NetHttp {
        host: String,
        port: Option<u16>,
        scheme: Option<String>,
        methods: Vec<String>,
    },
    BrowserUse {
        scope: String,
    },
    ProcExec {
        tool: String,
    },
    TmpUse,
    SysGpu {
        class: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CapabilityKind {
    FsRead,
    FsWrite,
    FsList,
    NetHttp,
    BrowserUse,
    ProcExec,
    TmpUse,
    SysGpu,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum SafetyClass {
    Low,
    Medium,
    High,
}

impl Default for SafetyClass {
    fn default() -> Self {
        SafetyClass::Low
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum SideEffect {
    None,
    Read,
    Write,
    Network,
    Filesystem,
    Browser,
    Process,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Grant {
    pub tenant: TenantId,
    pub subject_id: Id,
    pub tool_name: String,
    pub call_id: Id,
    pub capabilities: Vec<Capability>,
    pub expires_at: i64,
    pub budget: Budget,
    pub decision_fingerprint: String,
    #[serde(default)]
    pub consent: Option<Consent>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Budget {
    pub calls: u64,
    pub bytes_out: u64,
    pub bytes_in: u64,
    pub cpu_ms: u64,
    pub gpu_ms: u64,
    pub file_count: u64,
}

impl Budget {
    pub fn saturating_sub(&self, rhs: &Budget) -> Budget {
        Budget {
            calls: self.calls.saturating_sub(rhs.calls),
            bytes_out: self.bytes_out.saturating_sub(rhs.bytes_out),
            bytes_in: self.bytes_in.saturating_sub(rhs.bytes_in),
            cpu_ms: self.cpu_ms.saturating_sub(rhs.cpu_ms),
            gpu_ms: self.gpu_ms.saturating_sub(rhs.gpu_ms),
            file_count: self.file_count.saturating_sub(rhs.file_count),
        }
    }

    pub fn add_assign(&mut self, other: &Budget) {
        self.calls = self.calls.saturating_add(other.calls);
        self.bytes_out = self.bytes_out.saturating_add(other.bytes_out);
        self.bytes_in = self.bytes_in.saturating_add(other.bytes_in);
        self.cpu_ms = self.cpu_ms.saturating_add(other.cpu_ms);
        self.gpu_ms = self.gpu_ms.saturating_add(other.gpu_ms);
        self.file_count = self.file_count.saturating_add(other.file_count);
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Limits {
    pub max_bytes_in: Option<u64>,
    pub max_bytes_out: Option<u64>,
    pub max_files: Option<u64>,
    pub max_depth: Option<u32>,
    pub max_concurrency: Option<u32>,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_bytes_in: None,
            max_bytes_out: None,
            max_files: None,
            max_depth: None,
            max_concurrency: None,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Whitelists {
    pub domains: Vec<String>,
    pub paths: Vec<String>,
    pub tools: Vec<String>,
    pub mime_allow: Vec<String>,
    pub methods: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Mappings {
    pub root_fs: Option<String>,
    pub tmp_dir: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Profile {
    pub tenant: TenantId,
    pub subject_id: Id,
    pub tool_name: String,
    pub call_id: Id,
    pub capabilities: Vec<Capability>,
    pub safety: SafetyClass,
    pub side_effects: Vec<SideEffect>,
    pub limits: Limits,
    pub whitelists: Whitelists,
    pub mappings: Mappings,
    pub timeout_ms: u64,
    pub profile_hash: String,
    pub policy_hash: Option<String>,
    pub config_version: Option<String>,
    pub config_hash: Option<String>,
}

impl Profile {
    pub fn hash(&self) -> String {
        let mut hasher = Sha256::new();
        let json = serde_json::to_vec(&self).unwrap_or_default();
        hasher.update(json);
        let digest = hasher.finalize();
        hex::encode(digest)
    }
}

impl Capability {
    pub fn kind(&self) -> CapabilityKind {
        match self {
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

    pub fn describe(&self) -> String {
        match self {
            Capability::FsRead { path } => format!("fs.read:{}", path),
            Capability::FsWrite { path, append } => {
                format!("fs.write:{}:append={}", path, append)
            }
            Capability::FsList { path } => format!("fs.list:{}", path),
            Capability::NetHttp {
                host,
                port,
                scheme,
                methods,
            } => {
                let mut parts = vec![scheme.clone().unwrap_or_else(|| "http".to_string())];
                parts.push(host.clone());
                if let Some(port) = port {
                    parts.push(port.to_string());
                }
                if !methods.is_empty() {
                    parts.push(methods.join("|"));
                }
                format!("net.http:{}", parts.join(":"))
            }
            Capability::BrowserUse { scope } => format!("browser.use:{}", scope),
            Capability::ProcExec { tool } => format!("proc.exec:{}", tool),
            Capability::TmpUse => "tmp.use".to_string(),
            Capability::SysGpu { class } => format!("sys.gpu:{}", class),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ToolManifest {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub capabilities: Vec<Capability>,
    #[serde(default)]
    pub safety: SafetyClass,
    #[serde(default)]
    pub side_effects: Vec<SideEffect>,
    #[serde(default)]
    pub limits: Option<Limits>,
    #[serde(default)]
    pub whitelists: Option<Whitelists>,
    #[serde(default)]
    pub mappings: Option<Mappings>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub metadata: Value,
}

impl ToolManifest {
    pub fn safety(&self) -> SafetyClass {
        self.safety
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DataDigest {
    pub algo: String,
    pub b64: String,
    pub size: u64,
}

impl DataDigest {
    pub fn sha256(bytes: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let digest = hasher.finalize();
        Self {
            algo: "sha256".to_string(),
            b64: BASE64.encode(digest),
            size: bytes.len() as u64,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SideEffectRecord {
    pub kind: SideEffect,
    #[serde(default)]
    pub meta: Value,
}
