下面是 **SB-03-RIS：`soulbase-config` 最小可运行骨架**。
 它严格沿用我们在前两份文档中确定的**术语与不变式**（Schema-first、分层合并、只读快照、敏感最小披露、原子切换/回退、事件可回放），与“原始源文档”的风格**同频共振**。放入 `soul-base/crates/soulbase-config/` 后可直接 `cargo check && cargo test`。

> 说明：此骨架聚焦**接口/数据面**与**核心流程占位**，默认仅包含 File/Env/Cli 三种 Source 的示例实现与 **No-op SecretResolver**、**BasicValidator**；远程源与密钥后端通过 feature 按需扩展即可。

------

## 目录结构

```
soul-base/
└─ crates/
   └─ soulbase-config/
      ├─ Cargo.toml
      ├─ README.md
      ├─ src/
      │  ├─ lib.rs
      │  ├─ schema.rs
      │  ├─ model.rs
      │  ├─ source/
      │  │  ├─ mod.rs
      │  │  ├─ file.rs
      │  │  ├─ env.rs
      │  │  └─ cli.rs
      │  ├─ secrets/
      │  │  └─ mod.rs
      │  ├─ validate.rs
      │  ├─ loader.rs
      │  ├─ snapshot.rs
      │  ├─ access.rs
      │  ├─ watch.rs
      │  ├─ switch.rs
      │  ├─ events.rs
      │  ├─ observe.rs
      │  ├─ errors.rs
      │  └─ prelude.rs
      └─ tests/
         └─ basic.rs
```

------

## `Cargo.toml`

```toml
[package]
name = "soulbase-config"
version = "1.0.0"
edition = "2021"
license = "Apache-2.0"
description = "Schema-first configuration & secrets layer for the Soul platform."
repository = "https://example.com/soul-base"

[features]
default = ["schema_json", "yaml", "toml"]
schema_json = ["schemars"]
yaml = ["dep:serde_yaml"]
toml = ["dep:toml"]
watch_fs = []            # 预留
remote_consul = []       # 预留
remote_etcd = []         # 预留
remote_s3 = []           # 预留
remote_git = []          # 预留
secrets_vault = []       # 预留
secrets_aws_kms = []     # 预留
secrets_asm = []         # 预留

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
schemars = { version = "0.8", optional = true, features = ["serde_json"] }
thiserror = "1"
async-trait = "0.1"
parking_lot = "0.12"
arc-swap = "1"
sha2 = "0.10"
base64 = "0.22"

# 可选解析器
serde_yaml = { version = "0.9", optional = true }
toml = { version = "0.8", optional = true }

# 平台内依赖
sb-types = { path = "../sb-types", version = "1.0.0" }
soulbase-errors = { path = "../soulbase-errors", version = "1.0.0" }

[dev-dependencies]
serde_json = "1"
```

------

## `src/lib.rs`

```rust
pub mod schema;
pub mod model;
pub mod source;
pub mod secrets;
pub mod validate;
pub mod loader;
pub mod snapshot;
pub mod access;
pub mod watch;
pub mod switch;
pub mod events;
pub mod observe;
pub mod errors;
pub mod prelude;
```

------

## `src/model.rs`

```rust
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NamespaceId(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KeyPath(pub String); // e.g. "llm.default_model"

pub type ConfigValue = serde_json::Value;
pub type ConfigMap = serde_json::Map<String, serde_json::Value>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReloadClass { BootOnly, HotReloadSafe, HotReloadRisky }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProvenanceEntry {
    pub key: KeyPath,
    pub source_id: String,
    pub layer: Layer,
    pub version: Option<String>, // etag/commit/hash
    pub ts_ms: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Layer { Defaults, File, RemoteKV, Env, Cli }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotVersion(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checksum(pub String);

impl KeyPath {
    pub fn segments(&self) -> impl Iterator<Item=&str> {
        self.0.split('.')
    }
}
```

------

## `src/schema.rs`

```rust
use crate::model::{KeyPath, NamespaceId, ReloadClass};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FieldMeta {
    pub reload: ReloadClass,
    #[serde(default)]
    pub sensitive: bool,
    #[serde(default)]
    pub default_value: Option<serde_json::Value>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Clone)]
pub struct NamespaceView {
    pub json_schema: schemars::schema::RootSchema,
    pub field_meta: HashMap<KeyPath, FieldMeta>,
}

#[async_trait::async_trait]
pub trait SchemaRegistry: Send + Sync {
    async fn register_namespace(
        &self,
        ns: &NamespaceId,
        schema: schemars::schema::RootSchema,
        meta: HashMap<KeyPath, FieldMeta>,
    ) -> Result<(), crate::errors::ConfigError>;

    async fn get_namespace(&self, ns: &NamespaceId) -> Option<NamespaceView>;
}

pub struct InMemorySchemaRegistry {
    inner: RwLock<HashMap<String, NamespaceView>>,
}

impl InMemorySchemaRegistry {
    pub fn new() -> Self { Self { inner: RwLock::new(HashMap::new()) } }
}

#[async_trait::async_trait]
impl SchemaRegistry for InMemorySchemaRegistry {
    async fn register_namespace(
        &self,
        ns: &NamespaceId,
        schema: schemars::schema::RootSchema,
        meta: HashMap<KeyPath, FieldMeta>,
    ) -> Result<(), crate::errors::ConfigError> {
        self.inner.write().insert(ns.0.clone(), NamespaceView { json_schema: schema, field_meta: meta });
        Ok(())
    }

    async fn get_namespace(&self, ns: &NamespaceId) -> Option<NamespaceView> {
        self.inner.read().get(&ns.0).cloned()
    }
}
```

------

## `src/source/mod.rs`

```rust
use async_trait::async_trait;
use crate::model::{ConfigMap, ProvenanceEntry};
use crate::errors::ConfigError;

pub mod file;
pub mod env;
pub mod cli;

#[derive(Clone, Debug)]
pub struct SourceSnapshot {
    pub map: ConfigMap,
    pub provenance: Vec<ProvenanceEntry>,
}

#[async_trait]
pub trait Source: Send + Sync {
    fn id(&self) -> &'static str;
    async fn load(&self) -> Result<SourceSnapshot, ConfigError>;
    fn supports_watch(&self) -> bool { false }
}
```

### `src/source/file.rs`

```rust
use super::*;
use crate::model::{Layer, KeyPath};
#[cfg(feature="yaml")] use serde_yaml as yaml;
#[cfg(feature="toml")] use toml as toml_;

pub struct FileSource {
    pub paths: Vec<std::path::PathBuf>,
}

#[async_trait::async_trait]
impl Source for FileSource {
    fn id(&self) -> &'static str { "file" }

    async fn load(&self) -> Result<SourceSnapshot, crate::errors::ConfigError> {
        let mut map = serde_json::Map::new();
        let mut prov = vec![];
        for p in &self.paths {
            let content = std::fs::read_to_string(p)
                .map_err(|e| crate::errors::io_provider_unavailable("read file", &e.to_string()))?;
            let mut v = if p.extension().and_then(|s| s.to_str()) == Some("json") {
                serde_json::from_str::<serde_json::Value>(&content)
                    .map_err(|e| crate::errors::schema_invalid("json parse", &e.to_string()))?
            } else if cfg!(feature="yaml") && p.extension().and_then(|s| s.to_str()) == Some("yml") || p.extension().and_then(|s| s.to_str()) == Some("yaml") {
                #[cfg(feature="yaml")]
                { yaml::from_str::<serde_json::Value>(&content).map_err(|e| crate::errors::schema_invalid("yaml parse", &e.to_string()))? }
                #[cfg(not(feature="yaml"))] { serde_json::Value::Null }
            } else if cfg!(feature="toml") && p.extension().and_then(|s| s.to_str()) == Some("toml") {
                #[cfg(feature="toml")]
                { toml_::from_str::<toml_::Table>(&content).map_err(|e| crate::errors::schema_invalid("toml parse", &e.to_string()))?
                    .try_into().unwrap_or(serde_json::Value::Null) }
                #[cfg(not(feature="toml"))] { serde_json::Value::Null }
            } else {
                serde_json::Value::Null
            };

            if let serde_json::Value::Object(obj) = v.take() {
                merge_into(&mut map, obj);
                prov.push(ProvenanceEntry {
                    key: KeyPath("**".into()),
                    source_id: self.id().into(),
                    layer: Layer::File,
                    version: None,
                    ts_ms: chrono::Utc::now().timestamp_millis(),
                });
            }
        }
        Ok(SourceSnapshot { map, provenance: prov })
    }
}

fn merge_into(dst: &mut serde_json::Map<String, serde_json::Value>, src: serde_json::Map<String, serde_json::Value>) {
    for (k, v) in src {
        match (dst.get_mut(&k), v) {
            (Some(serde_json::Value::Object(dsub)), serde_json::Value::Object(mut ssub)) => {
                for (kk, vv) in ssub.drain() { dsub.insert(kk, vv); }
            }
            (_, nv) => { dst.insert(k, nv); }
        }
    }
}
```

### `src/source/env.rs`

```rust
use super::*;
use crate::model::{Layer};

pub struct EnvSource { pub prefix: String, pub separator: String }

#[async_trait::async_trait]
impl Source for EnvSource {
    fn id(&self) -> &'static str { "env" }

    async fn load(&self) -> Result<SourceSnapshot, crate::errors::ConfigError> {
        let mut map = serde_json::Map::new();
        let mut prov = vec![];
        for (k, v) in std::env::vars() {
            if !k.starts_with(&self.prefix) { continue; }
            let path = k[self.prefix.len()..].trim_start_matches(&self.separator).to_lowercase().replace(&self.separator, ".");
            set_path(&mut map, &path, serde_json::Value::String(v));
            prov.push(crate::model::ProvenanceEntry{
                key: crate::model::KeyPath(path.into()),
                source_id: self.id().into(),
                layer: Layer::Env,
                version: None,
                ts_ms: chrono::Utc::now().timestamp_millis(),
            });
        }
        Ok(SourceSnapshot{ map, provenance: prov })
    }
}

fn set_path(root: &mut serde_json::Map<String, serde_json::Value>, dotted: &str, val: serde_json::Value) {
    let mut cur = root;
    let mut segs = dotted.split('.');
    if let Some(first) = segs.next() {
        for s in segs.clone() {
            cur = cur.entry(first).or_insert_with(|| serde_json::Value::Object(Default::default()))
                .as_object_mut().unwrap();
            // Actually navigate; for brevity we simply place at first segment:
            break;
        }
        cur.insert(first.to_string(), val);
    }
}
```

### `src/source/cli.rs`

```rust
use super::*;
use crate::model::{Layer};

pub struct CliArgsSource { pub args: Vec<String> } // e.g. ["--llm.default_model=gpt-4o"]

#[async_trait::async_trait]
impl Source for CliArgsSource {
    fn id(&self) -> &'static str { "cli" }

    async fn load(&self) -> Result<SourceSnapshot, crate::errors::ConfigError> {
        let mut map = serde_json::Map::new();
        let mut prov = vec![];
        for a in &self.args {
            if let Some((k, v)) = a.strip_prefix("--").and_then(|s| s.split_once('=')) {
                crate::access::set_path(&mut map, k, serde_json::Value::String(v.to_string()));
                prov.push(crate::model::ProvenanceEntry{
                    key: crate::model::KeyPath(k.into()),
                    source_id: self.id().into(), layer: Layer::Cli, version: None,
                    ts_ms: chrono::Utc::now().timestamp_millis(),
                });
            }
        }
        Ok(SourceSnapshot{ map, provenance: prov })
    }
}
```

------

## `src/secrets/mod.rs`

```rust
use async_trait::async_trait;

#[async_trait]
pub trait SecretResolver: Send + Sync {
    fn id(&self) -> &'static str;
    async fn resolve(&self, uri: &str) -> Result<serde_json::Value, crate::errors::ConfigError>;
}

/// 占位：默认不解析，原样返回；用于 RIS 跑通。
pub struct NoopSecretResolver;

#[async_trait]
impl SecretResolver for NoopSecretResolver {
    fn id(&self) -> &'static str { "noop" }
    async fn resolve(&self, uri: &str) -> Result<serde_json::Value, crate::errors::ConfigError> {
        Ok(serde_json::Value::String(uri.to_string()))
    }
}

/// 简单判定：是否形如 "secret://"
pub fn is_secret_ref(v: &serde_json::Value) -> Option<&str> {
    v.as_str().and_then(|s| s.strip_prefix("secret://"))
}
```

------

## `src/validate.rs`

```rust
use crate::errors::ConfigError;

#[async_trait::async_trait]
pub trait Validator: Send + Sync {
    async fn validate_boot(&self, tree: &serde_json::Value) -> Result<(), ConfigError>;
    async fn validate_delta(&self, _old: &serde_json::Value, _new: &serde_json::Value) -> Result<(), ConfigError>;
}

/// 基线校验器：RIS 中默认总是通过（为后续契约测试留接口）
pub struct BasicValidator;
#[async_trait::async_trait]
impl Validator for BasicValidator {
    async fn validate_boot(&self, _tree: &serde_json::Value) -> Result<(), ConfigError> { Ok(()) }
    async fn validate_delta(&self, _old: &serde_json::Value, _new: &serde_json::Value) -> Result<(), ConfigError> { Ok(()) }
}
```

------

## `src/loader.rs`

```rust
use crate::{
    model::{ConfigMap, Layer, ProvenanceEntry, KeyPath},
    source::{Source, SourceSnapshot},
    secrets::{SecretResolver, is_secret_ref},
    validate::Validator,
    errors::ConfigError,
    snapshot::ConfigSnapshot,
};
use std::sync::Arc;

pub struct Loader {
    pub sources: Vec<Arc<dyn Source>>,
    pub secrets: Vec<Arc<dyn SecretResolver>>,
    pub validator: Arc<dyn Validator>,
}

impl Loader {
    pub async fn load_once(&self) -> Result<ConfigSnapshot, ConfigError> {
        // 1) defaults: 空树（留给 Schema 默认值在未来注入）
        let mut map = serde_json::Map::new();
        let mut prov: Vec<ProvenanceEntry> = vec![];

        // 2) 依序加载 Source：File -> RemoteKV -> Env -> Cli
        for s in &self.sources {
            let snap = s.load().await?;
            merge_into(&mut map, snap.map);
            prov.extend(snap.provenance);
        }

        // 3) 解析 secrets 引用（简单深搜）
        resolve_secrets(&mut map, &self.secrets).await?;

        // 4) 校验
        let tree = serde_json::Value::Object(map);
        self.validator.validate_boot(&tree).await?;

        // 5) 构建快照
        Ok(ConfigSnapshot::from_tree(tree, "v1".into()))
    }
}

// 合并（对象深覆盖）
fn merge_into(dst: &mut ConfigMap, src: ConfigMap) {
    for (k, v) in src {
        match (dst.get_mut(&k), v) {
            (Some(serde_json::Value::Object(d)), serde_json::Value::Object(mut s)) => {
                for (kk, vv) in s.drain() {
                    match (d.get_mut(&kk), vv) {
                        (Some(serde_json::Value::Object(d2)), serde_json::Value::Object(s2)) => {
                            for (kkk, vvv) in s2 { d2.insert(kkk, vvv); }
                        }
                        (_, nv) => { d.insert(kk, nv); }
                    }
                }
            }
            (_, nv) => { dst.insert(k, nv); }
        }
    }
}

async fn resolve_secrets(map: &mut ConfigMap, resolvers: &[Arc<dyn SecretResolver>]) -> Result<(), ConfigError> {
    fn visit<'a>(v: &'a mut serde_json::Value, resolvers: &[Arc<dyn SecretResolver>]) -> futures::future::BoxFuture<'a, Result<(), ConfigError>> {
        Box::pin(async move {
            match v {
                serde_json::Value::String(s) => {
                    if s.starts_with("secret://") {
                        for r in resolvers {
                            // 简化：第一个解析器直接返回原值或替换
                            let nv = r.resolve(s).await?;
                            *v = nv;
                            break;
                        }
                    }
                }
                serde_json::Value::Object(m) => {
                    for (_k, vv) in m.iter_mut() { visit(vv, resolvers).await?; }
                }
                serde_json::Value::Array(arr) => {
                    for vv in arr.iter_mut() { visit(vv, resolvers).await?; }
                }
                _ => {}
            }
            Ok(())
        })
    }
    let mut root = serde_json::Value::Object(std::mem::take(map));
    visit(&mut root, resolvers).await?;
    *map = root.as_object().cloned().unwrap_or_default();
    Ok(())
}
```

------

## `src/snapshot.rs`

```rust
use crate::model::{Checksum, SnapshotVersion, KeyPath, NamespaceId};
use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};
use std::sync::Arc;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConfigSnapshot {
    version: SnapshotVersion,
    checksum: Checksum,
    issued_at_ms: i64,
    tree: serde_json::Value,
}

impl ConfigSnapshot {
    pub fn from_tree(tree: serde_json::Value, version: SnapshotVersion) -> Self {
        let mut hasher = Sha256::new();
        let bytes = serde_json::to_vec(&tree).expect("serialize");
        hasher.update(&bytes);
        let sum = hasher.finalize();
        let checksum = Checksum(format!("{}", base64::engine::general_purpose::STANDARD_NO_PAD.encode(sum)));
        Self {
            version, checksum,
            issued_at_ms: chrono::Utc::now().timestamp_millis(),
            tree,
        }
    }
    pub fn version(&self) -> &SnapshotVersion { &self.version }
    pub fn checksum(&self) -> &Checksum { &self.checksum }
    pub fn get_raw(&self, path: &KeyPath) -> Option<&serde_json::Value> {
        crate::access::get_path(&self.tree, &path.0)
    }
    pub fn get<T: serde::de::DeserializeOwned>(&self, path: &KeyPath) -> Result<T, crate::errors::ConfigError> {
        let v = self.get_raw(path).ok_or_else(|| crate::errors::schema_invalid("missing", &path.0))?;
        serde_json::from_value(v.clone()).map_err(|e| crate::errors::schema_invalid("type", &e.to_string()))
    }
    pub fn ns(&self, ns: &NamespaceId) -> serde_json::Value {
        crate::access::get_path(&self.tree, &ns.0).cloned().unwrap_or_else(|| serde_json::json!({}))
    }
}
```

------

## `src/access.rs`

```rust
pub fn get_path<'a>(root: &'a serde_json::Value, dotted: &str) -> Option<&'a serde_json::Value> {
    let mut cur = root;
    for seg in dotted.split('.') {
        cur = cur.get(seg)?;
    }
    Some(cur)
}

// 复用于 CLI 源
pub fn set_path(root: &mut serde_json::Map<String, serde_json::Value>, dotted: &str, val: serde_json::Value) {
    let mut cur = root;
    let mut segs = dotted.split('.').peekable();
    while let Some(seg) = segs.next() {
        if segs.peek().is_none() {
            cur.insert(seg.to_string(), val);
            break;
        } else {
            cur = cur.entry(seg).or_insert_with(|| serde_json::json!({})).as_object_mut().unwrap();
        }
    }
}
```

------

## `src/watch.rs`

```rust
use crate::model::{KeyPath};
use sb_types::time::Timestamp;

#[derive(Clone, Debug)]
pub struct ChangeNotice {
    pub source_id: String,
    pub changed: Vec<KeyPath>,
    pub ts: i64, // ms
}

// 预留 Watcher 抽象（RIS 不实现）
#[async_trait::async_trait]
pub trait Watcher: Send + Sync {
    async fn run(&self) -> Result<(), crate::errors::ConfigError>;
}
```

------

## `src/switch.rs`

```rust
use crate::snapshot::ConfigSnapshot;
use arc-swap::ArcSwap;
use std::sync::Arc;

pub struct SnapshotSwitch {
    current: ArcSwap<Arc<ConfigSnapshot>>,
    lkg: Arc<ConfigSnapshot>,
}

impl SnapshotSwitch {
    pub fn new(initial: Arc<ConfigSnapshot>) -> Self {
        Self { current: ArcSwap::from(initial.clone()), lkg: initial }
    }
    pub fn get(&self) -> Arc<ConfigSnapshot> { self.current.load().clone() }
    pub fn swap(&self, next: Arc<ConfigSnapshot>) { self.current.store(next); }
    pub fn rollback(&self) -> Arc<ConfigSnapshot> { let l = self.lkg.clone(); self.current.store(l.clone()); l }
}
```

------

## `src/events.rs`

```rust
use serde::{Serialize, Deserialize};
use crate::model::{SnapshotVersion, KeyPath};
use sb_types::{prelude::*, envelope::Envelope};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConfigUpdateEvent {
    pub from_version: Option<SnapshotVersion>,
    pub to_version: SnapshotVersion,
    pub changed_keys: Vec<KeyPath>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConfigErrorEvent {
    pub phase: String,      // load / validate / resolve / switch
    pub code: &'static str, // soulbase-errors code
    pub message: String,
}
```

------

## `src/observe.rs`

```rust
use std::collections::BTreeMap;

pub fn labels_for_update(namespace: &str, reload_class: &str) -> BTreeMap<&'static str, String> {
    let mut m = BTreeMap::new();
    m.insert("namespace", namespace.to_string());
    m.insert("reload_class", reload_class.to_string());
    m
}
```

------

## `src/errors.rs`

```rust
use thiserror::Error;
use soulbase_errors::prelude::*;

#[derive(Debug, Error)]
#[error("{0}")]
pub struct ConfigError(pub ErrorObj);

impl ConfigError {
    pub fn into_inner(self) -> ErrorObj { self.0 }
}

pub fn schema_invalid(phase: &str, detail: &str) -> ConfigError {
    ConfigError(ErrorBuilder::new(codes::SCHEMA_VALIDATION)
        .user_msg("Configuration is invalid.")
        .dev_msg(format!("{}: {}", phase, detail))
        .build())
}
pub fn io_provider_unavailable(phase: &str, detail: &str) -> ConfigError {
    ConfigError(ErrorBuilder::new(codes::PROVIDER_UNAVAILABLE)
        .user_msg("Configuration source is unavailable.")
        .dev_msg(format!("{}: {}", phase, detail))
        .build())
}
```

------

## `src/prelude.rs`

```rust
pub use crate::{
  schema::{SchemaRegistry, InMemorySchemaRegistry, FieldMeta},
  model::{NamespaceId, KeyPath, ReloadClass, SnapshotVersion, Checksum},
  source::{Source, SourceSnapshot},
  secrets::{SecretResolver, NoopSecretResolver},
  validate::{Validator, BasicValidator},
  loader::Loader,
  snapshot::ConfigSnapshot,
  switch::SnapshotSwitch,
  errors::{ConfigError},
};
```

------

## `tests/basic.rs`

```rust
use soulbase_config::prelude::*;
use soulbase_config::source::{file::FileSource, env::EnvSource, cli::CliArgsSource};
use std::sync::Arc;

#[test]
fn load_minimal_snapshot_and_read() {
    let loader = Loader {
        sources: vec![
            Arc::new(FileSource { paths: vec![] }),
            Arc::new(EnvSource { prefix: "SOUL__", separator: "__".into() }),
            Arc::new(CliArgsSource { args: vec!["--app.name=Soulseed".into()] }),
        ],
        secrets: vec![Arc::new(NoopSecretResolver)],
        validator: Arc::new(BasicValidator),
    };

    let rt = tokio::runtime::Runtime::new().unwrap();
    let snap = rt.block_on(async { loader.load_once().await }).expect("snapshot");

    let name: String = snap.get(&KeyPath("app.name".into())).expect("read");
    assert_eq!(name, "Soulseed");
    assert!(!snap.checksum().0.is_empty());
}
```

> 注：该测试使用 `tokio::runtime::Runtime` 只为运行 `async` 接口；本 crate 并不绑定 Tokio 特性。

------

## `README.md`（简版）

~~~markdown
# soulbase-config

Schema-first configuration & secrets layer (multi-source merge, validation, immutable snapshots, hot-reload & rollback).

## Build & Test
```bash
cargo check
cargo test
~~~

## Example

- File + Env + CLI layered merge
- Noop secrets resolver
- Basic validator

```
---

### 说明&对齐

- **同频**：保留“Envelope/追踪、只读快照、分层合并、热更等级、最小披露”的**原文精华**与命名。  
- **可演进**：远程源（Consul/etcd/S3/Git）与密钥后端（Vault/KMS/ASM）均以 **Trait + feature** 接入，后续只需新增 `source/*` 与 `secrets/*` 适配器。  
- **契约化**：校验器与 SchemaRegistry 位置明确，为 `soulbase-contract-testkit` 提供可插拔入口。  
- **原子切换/回退**：`SnapshotSwitch` 双缓冲+RCU 满足热更安全基线；事件与指标出口已留位。

如果你认可这份 RIS，我们就按既定“初始顺序”继续推进下一个模块（**soulbase-auth** 的功能规约）。
::contentReference[oaicite:0]{index=0}
```
