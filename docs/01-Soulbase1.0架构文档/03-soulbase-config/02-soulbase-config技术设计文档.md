# 文档 SB-03-TD：`soulbase-config` 技术设计（Technical Design）

> 对应功能规约：SB-03（配置与密钥管理 / Configuration & Secrets）
>  目标：给出 **crate 结构、Source/Watcher/Resolver/Validator 抽象、快照模型、变更事件、热更原子切换与回退机制、Secrets 接口** 的可落地设计。
>  语言：Rust（以 `serde`/`serde_json` 为基础；JSON-Schema 为首选契约）。
>  说明：本 TD **不包含 RIS 代码骨架**，仅给出接口草案与行为约束，RIS 将在下一步单独输出。

------

## 1. 设计目标与非目标（摘录）

- **目标**：建立一个 **Schema-first、可审计、可回滚** 的配置/密钥层，覆盖多源合并、类型化读取、热更新与密钥轮转。
- **非目标**：不嵌入业务策略；不绑定具体远程后端（以 **Trait + 适配器** 方式接入）；不将敏感值写日志/导出。

------

## 2. Crate 结构与模块划分

```
soulbase-config/
  src/
    lib.rs
    schema.rs            # 命名空间Schema注册、默认值、字段级元数据（ReloadClass等）
    model.rs             # 基础类型（KeyPath, NamespaceId, ConfigValue, ConfigMap, Provenance…）
    source/mod.rs        # Source 抽象与内置适配（File/Env/Cli/RemoteKV/SecretRef）
      file.rs
      env.rs
      cli.rs
      remotekv.rs        # etcd/consul/s3/git 等以 feature 启用
    secrets/mod.rs       # SecretResolver 抽象与适配（Vault/KMS/Secrets Manager…）
    validate.rs          # Schema & 业务无关校验器（结构、范围、必填）
    loader.rs            # 多源加载/合并/解引用流水线
    snapshot.rs          # ConfigSnapshot（不可变视图）、版本/校验和/读取接口
    access.rs            # 命名空间访问裁剪、类型化Getter、FeatureFlag读取口径
    watch.rs             # Watcher 抽象、热更订阅、抖动抑制、合并通知
    switch.rs            # 原子切换/双缓冲/回退(LKG)机制
    events.rs            # Envelope<ConfigUpdateEvent/ConfigErrorEvent> 事件定义
    observe.rs           # 指标/计时/错误标签导出（对接 soulbase-observe）
    errors.rs            # 与 soulbase-errors 的映射（SCHEMA/PROVIDER/AUTH…）
    prelude.rs
```

**Feature flags（建议）**

- `schema_json`（默认）: 开启 JSON-Schema 生成功能（依赖 `schemars`）。
- `remote_consul` / `remote_etcd` / `remote_s3` / `remote_git`：远程配置源适配。
- `secrets_vault` / `secrets_aws_kms` / `secrets_asm`：密钥后端适配。
- `watch_fs` / `watch_remote`：文件或远程源监听。
- `toml` / `yaml`: 文件解析格式支持。

------

## 3. 核心类型（`model.rs` 概念草案）

- `NamespaceId(pub String)`: 命名空间（如 `server`, `auth`, `llm`）。
- `KeyPath(pub String)`: 点分路径（`llm.default_model`）。
- `ConfigValue = serde_json::Value`: 统一值表示。
- `ConfigMap = serde_json::Map<String, Value>`: 扁平或树形配置。
- `ReloadClass = BootOnly | HotReloadSafe | HotReloadRisky`: 热更等级。
- `Provenance`: 每个键的来源链（`source_id`, `version/etag`, `layer`, `ts`）。
- `Checksum(pub String)`: 规范化 JSON 的哈希（建议 `SHA256`）。
- `SnapshotVersion(pub String)`: 语义化版本/递增序列（与 Schema 版本解耦）。
- `SnapshotId = (SnapshotVersion, Checksum)`。

------

## 4. Schema 注册与字段元数据（`schema.rs`）

### 4.1 SchemaRegistry

- 作用：注册**命名空间 Schema**与**字段元数据**（默认值、ReloadClass、敏感级别、取值范围/枚举）。
- 行为不变式：
  - 未注册键**不得出现在最终快照**。
  - `ReloadClass` 决定热更能力；`Sensitive` 决定输出与审计策略。

### 4.2 接口草案

```rust
pub struct FieldMeta {
  pub reload: ReloadClass,
  pub sensitive: bool,
  pub default_value: Option<serde_json::Value>,
  pub description: Option<String>,
}

pub trait SchemaRegistry: Send + Sync {
  fn register_namespace(
    &self,
    ns: &NamespaceId,
    json_schema: schemars::schema::RootSchema,
    field_meta: &std::collections::HashMap<KeyPath, FieldMeta>
  ) -> Result<(), Error>; // Error 映射见 12节
  fn get_namespace(&self, ns: &NamespaceId) -> Option<NamespaceView>;
}
```

------

## 5. Source 抽象与适配（`source/`）

### 5.1 概念

- **Source**：配置来源（File/Env/Cli/RemoteKV）。
- **Layer**：加载/合并顺序：`Defaults < File < RemoteKV < Env < Cli`。
- **输出**：每个 Source 返回一个**部分树**（`ConfigMap`）及其 `Provenance`。

### 5.2 Trait 接口

```rust
#[derive(Clone, Debug)]
pub struct SourceSnapshot {
  pub map: serde_json::Map<String, serde_json::Value>,
  pub provenance: Vec<ProvenanceEntry>,
}

#[async_trait::async_trait]
pub trait Source: Send + Sync {
  fn id(&self) -> &'static str;           // "file", "env", "cli", "remote:consul"
  async fn load(&self) -> Result<SourceSnapshot, Error>;
  fn supports_watch(&self) -> bool { false }
  async fn watch(&self, _tx: WatchTx) -> Result<(), Error> { Ok(()) } // 可选
}
```

**内置适配（示意）**

- `FileSource{ paths: Vec<PathBuf>, format: Json|Yaml|Toml }`
- `EnvSource{ prefix: String, separator: "__" }`
- `CliArgsSource{ args: Vec<String> }`
- `RemoteKVSource{ backend: RemoteBackend, base_path: String }`

> `RemoteBackend` 通过 `enum` + `feature` 注入：`Consul`, `Etcd`, `S3`, `Git`.

------

## 6. Secret 引用与解密（`secrets/`）

### 6.1 概念

- **Secret 引用语法**（示例）：`secret://vault/path/to/entry#key`、`kms://arn:...`
- **敏感键规则**：`*.secret|*.password|*.token|*.key` 或在 Schema `FieldMeta.sensitive=true`。

### 6.2 Trait 接口

```rust
#[async_trait::async_trait]
pub trait SecretResolver: Send + Sync {
  fn id(&self) -> &'static str; // "vault", "aws_kms", "asm"
  async fn resolve(&self, uri: &str) -> Result<serde_json::Value, Error>; // 返回解密后的 Value
  // 可选：批量解析 & 轮转通知
}
```

- 由 `loader` 在**合并前/后**阶段对值进行解析：
  - **引用替换**：`"secret://..."` → 实际值（仍标记“敏感”，禁止外泄）。
  - **轮转**：`SecretResolver` 可提供 watcher/ttl，触发受控刷新。

------

## 7. 校验器（`validate.rs`）

### 7.1 责任

- **结构与类型**：对合并后的树做 **JSON-Schema** 校验。
- **字段级规则**：对范围、枚举、必填与依赖关系进行验证。
- **热更限制**：在热更路径上，验证更新集仅包含 `HotReload*` 键。

### 7.2 接口草案

```rust
pub trait Validator: Send + Sync {
  fn validate_boot(&self, tree: &serde_json::Value) -> Result<(), Error>;
  fn validate_delta(&self, old: &serde_json::Value, new: &serde_json::Value) -> Result<(), Error>;
}

pub struct ValidationReport { pub errors: Vec<ValidateItem>, pub warnings: Vec<String> }
pub struct ValidateItem { pub path: KeyPath, pub code: &'static str, pub msg: String }
```

------

## 8. Loader（多源合并/解引用流水线，`loader.rs`）

### 8.1 合并算法（规范）

1. 初始化为 **Defaults**（来自 `SchemaRegistry` 的 `default_value` 聚合）。
2. 依序调用 Sources：`File → RemoteKV → Env → CliArgs`。
3. 每步做 **结构合并**（深度覆盖）并记录 `Provenance`。
4. 对敏感字段执行 **SecretResolver.resolve**。
5. 生成 `tree_final` 后运行 `Validator.validate_boot()`。
6. 成功 → 产出 `ConfigSnapshot`；失败 → 返回标准错误。

### 8.2 接口草案

```rust
pub struct Loader {
  pub sources: Vec<Box<dyn Source>>,
  pub secrets: Vec<Box<dyn SecretResolver>>,
  pub validator: Box<dyn Validator>,
  pub registry: Arc<dyn SchemaRegistry>,
}

impl Loader {
  pub async fn load_once(&self) -> Result<ConfigSnapshot, Error>;
  pub async fn load_with(&self, overrides: serde_json::Value) -> Result<ConfigSnapshot, Error>; // 测试/灰度辅助
}
```

------

## 9. ConfigSnapshot（不可变视图，`snapshot.rs`）

### 9.1 结构

- `version: SnapshotVersion`
- `checksum: Checksum`（对**规范化树**计算）
- `provenance: Vec<ProvenanceEntry>`
- `issued_at: Timestamp`
- `reload_policy: ReloadPolicy`（集群热更策略摘要）
- `tree: serde_json::Value`（只读）

### 9.2 接口

```rust
pub struct ConfigSnapshot { /* fields */ }

impl ConfigSnapshot {
  pub fn version(&self) -> &SnapshotVersion;
  pub fn checksum(&self) -> &Checksum;
  pub fn get_raw(&self, path: &KeyPath) -> Option<&serde_json::Value>;

  pub fn get<T: serde::de::DeserializeOwned>(&self, path: &KeyPath) -> Result<T, Error>;
  pub fn ns(&self, ns: &NamespaceId) -> NamespaceView; // 裁剪命名空间
}
```

------

## 10. 访问接口与 FeatureFlag（`access.rs`）

- **类型化读取**：`get<T>(path)`，错误映射到 `SCHEMA.TYPE_MISMATCH` 等。
- **命名空间裁剪**：结合 `soulbase-auth` 的 Subject/Scope，在上层网关/拦截器做 **ACL 裁剪**；本模块提供裁剪工具。
- **FeatureFlag**：提供通用结构（`bool` / `variant` / `percentage` + audience），本模块只负责读取与解析。

------

## 11. Watcher 与热更（`watch.rs`）

### 11.1 抽象

- `Watcher` 监听支持的 Source（`supports_watch()` 为 true）。
- 采用 **抖动抑制（debounce）** 与**批量合并**避免风暴。
- 发出 `ChangeNotice{ changed_keys, source_id, ts }`。

```rust
pub struct ChangeNotice { pub source_id: String, pub changed: Vec<KeyPath>, pub ts: Timestamp }

#[async_trait::async_trait]
pub trait Watcher: Send + Sync {
  async fn run(&self, tx: WatchTx) -> Result<(), Error>;
}
```

> 参考实现：文件系统采用 `notify`；远程采用后端原生 Watch API 或轮询。

------

## 12. 原子切换与回退（`switch.rs`）

### 12.1 机制

- **双缓冲 + RCU**：
  - 当前快照持有在 `ArcSwap<ConfigSnapshot>`（或等效 RCU）；
  - 热更成功后 **原子替换指针**，旧快照延迟释放。
- **回退策略**：
  - 维护 **Last-Known-Good (LKG)**；热更失败或健康检查失败 → 回退至 LKG。
  - LKG 切换行为产生 `ConfigRollbackEvent`（审计）。

### 12.2 接口

```rust
pub struct SnapshotSwitch {
  current: arc_swap::ArcSwap<Arc<ConfigSnapshot>>,
  lkg: Arc<ConfigSnapshot>,
}

impl SnapshotSwitch {
  pub fn get(&self) -> Arc<ConfigSnapshot>;
  pub fn swap(&self, next: Arc<ConfigSnapshot>);   // 原子替换
  pub fn rollback(&self) -> Arc<ConfigSnapshot>;   // 回到 LKG
}
```

------

## 13. 事件与审计（`events.rs`）

- `ConfigUpdateEvent`：
  - `from_version`, `to_version`, `checksum_diff`, `changed_keys[]`, `provenance_summary`, `hotreload_class_summary`
- `ConfigErrorEvent`：
  - `phase`（load/validate/resolve/watch/switch）、`error.code`、`message_user`、`meta`
- **封装**：使用 `sb-types::Envelope<...>`，发送至观测管道（或事件总线）。

------

## 14. 观测与指标（`observe.rs`）

- **关键指标**：
  - `config_load_latency_ms`、`config_load_failures_total`
  - `config_hotreload_applied_total`、`config_hotreload_failed_total`
  - `secret_resolve_latency_ms`、`secret_resolve_failures_total`
- **标签**：`source_id`, `namespace`, `reload_class`, `reason`。
- 与 `soulbase-observe` 对接：导出统一标签 Map 与计时助手。

------

## 15. 错误与 `soulbase-errors` 映射（`errors.rs`）

- 加载失败：`PROVIDER.UNAVAILABLE`（远程/文件无法读取）、`NETWORK`。
- 解析失败：`SCHEMA.VALIDATION_FAILED`、`SCHEMA.TYPE_MISMATCH`。
- 鉴权失败（拉取远程/密钥）：`AUTH.UNAUTHENTICATED` / `AUTH.FORBIDDEN`。
- 热更违规（包含 BootOnly 键变更）：`POLICY.DENY_TOOL`（或定义 `POLICY.DENY_CONFIG_CHANGE` 子码）。
- 轮转失败：`SANDBOX.CAPABILITY_BLOCKED`/`PROVIDER.UNAVAILABLE`（视后端）。
- 回退：公共视图返回 `UNKNOWN.INTERNAL` 或具体子码，审计视图记录 `cause_chain`。

------

## 16. 并发与性能

- 启动加载目标：p95 < 2s（取决于远程源）；
- 合并与校验：树大小 ≤ 200KB p95 < 50ms（纯内存操作）；
- 轮询/监听：对远程源设置 **最小轮询间隔** 与 **反压**，避免雪崩。
- 秘密解析：支持**批量**接口以降低 RTT；必要时引入**本地密钥缓存 + TTL**（仅内存，谨慎）。

------

## 17. 安全与合规

- **敏感字段零日志**（结构化日志层面做字段屏蔽）；
- **密钥只留内存**；支持**进程内加密**（可选）与**硬件密钥**接口（KMS）；
- **最小披露**：命名空间访问裁剪；公共导出剔除敏感键；
- **权限**：远程源与密钥后端的访问凭据通过 `soulbase-auth` 下发/刷新（Token/证书）。

------

## 18. 测试与验收（契约/回放）

- **契约测试**（在 `soulbase-contract-testkit`）：
  - 分层合并 → 期望树一致（含 checksum）；
  - 未注册键拒绝；敏感键屏蔽；热更限制；
  - 错误码映射与公共/审计视图。
- **回放**：`ConfigUpdateEvent` 序列可重建历史快照链（用于故障复盘）。

------

## 19. 版本化与迁移

- **Schema 版本化**：命名空间 Schema 采用 SemVer；`MINOR` 仅允许新增可选字段；`MAJOR` 需要迁移指南。
- **快照版本**：与 Schema 独立递增；用于回滚与兼容判断。
- **弃用**：字段 `deprecated` 标注与过渡期双写/映射；在**两个 MINOR** 后移除。

------

## 20. 与外部系统协同（简述）

- **Soul-Auth**：远程源/密钥访问的 OIDC/JWT/STS 凭据；
- **Soul-Hub**：将快照 `version/checksum` 注入响应头，便于前后端对齐与排障；
- **SoulseedAGI 内核**：通过 `access.rs` 的类型化读取获取策略参数；`HotReloadSafe` 变化触发内核轻量刷新；`Risky` 变化通过灰度/旗标协调。

------

## 21. 开放问题（Open Issues）

- 多语言 SDK（TS/Go/Java）的 Schema 绑定与错误码一致性工具链。
- GitOps 流程（签名/双人复核/PR-based 变更）与运行时 Watch 的冲突化解（谁为最终真相源）。
- Secrets 轮转事件与长期连接（DB/LLM Provider）的**无损刷新**协议。
- `Risky` 热更的**分阶段发布**协议（百分比/租户白名单）是否在本模块提供最小实现或交由上层。

------

> 若上述 TD 符合你的预期，我将在下一步输出 **SB-03-RIS（最小可运行骨架）**，包含：模块目录、Trait 定义、基本实现占位、单测/契约测试钩子与示例配置/Schema。
