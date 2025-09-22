# 文档 SB-09-TD：`soulbase-storage` 技术设计（Technical Design）

> 对应规约：SB-09（存储抽象 + SurrealDB 适配）
>  目标：给出 **Storage SPI**、**SurrealDB 适配层接口与命名参数规则**、**迁移脚本规范**、**索引/向量检索抽象**、**错误与指标出口** 的可落地设计。
>  语言：Rust 接口草案（与 `sb-types / -errors / -observe / -qos` 同频）；示例以 SurrealQL 方言与官方 Rust 客户端习惯为参照。

------

## 1. Crate 结构与 Feature 规划

```
soulbase-storage/
  src/
    lib.rs
    spi/                    # 通用抽象：Datastore / Session / Tx / Repo / Graph / Search / Vector
      mod.rs
      query.rs              # 查询构建器（命名参数绑定）
      repo.rs               # 泛型仓储接口（T: Entity）
      graph.rs              # 边/遍历抽象
      search.rs             # 全文检索抽象
      vector.rs             # 向量检索抽象
      migrate.rs            # 迁移管理接口
      health.rs             # 健康与池化统计
    surreal/                # SurrealDB 适配（默认后端）
      mod.rs
      config.rs             # 连接/池/NS/DB/认证
      datastore.rs          # Datastore + Session 实现
      tx.rs                 # 显式事务
      binder.rs             # 命名参数绑定 & 租户强约束
      mapper.rs             # 抽象到 SurrealQL 的映射（Repo/Graph/Search/Vector）
      migrate.rs            # 迁移执行器
      errors.rs             # Surreal 错误到稳定码映射
      observe.rs            # 指标/审计出口
    model.rs                # Entity trait / Id 规范 / Page / Sort
    errors.rs               # StorageError（稳定码）
    observe.rs              # 指标与标签（通用）
    prelude.rs
```

**features**

- `vector`：开启向量索引/检索抽象
- `fulltext`：开启全文检索抽象
- `migrate`：迁移执行与校验
- `surreal-ws` / `surreal-http`：连接协议选择（可同时启用，自动优先 WS）
- `pool`：连接池（默认启用）
- `qos` / `observe`：与 QoS/Observe 的指标打点与预算计量

------

## 2. Storage SPI（`spi/`）

### 2.1 公共模型（`model.rs`）

```rust
use sb_types::prelude::*;

pub trait Entity: Sized + serde::de::DeserializeOwned + serde::ser::Serialize {
    const TABLE: &'static str;                // Surreal 表名
    type Key: ToString;                       // 记录 ID 的本地标识

    fn id(&self) -> &str;                     // "table:id" or "id"（由适配器规范化）
}

#[derive(Clone, Debug)]
pub struct Page<T> { pub items: Vec<T>, pub next: Option<String> } // 游标分页

#[derive(Clone, Debug)]
pub struct Sort { pub field: String, pub asc: bool }

pub fn make_record_id(table: &str, tenant: &TenantId, ulid: &str) -> String {
    format!("{table}:{}_{}", tenant.0, ulid)
}
```

### 2.2 Datastore / Session / Tx（`spi/mod.rs`）

```rust
#[async_trait::async_trait]
pub trait Datastore: Send + Sync {
    async fn session(&self) -> Result<Box<dyn Session>, StorageError>;
    async fn health(&self) -> Result<HealthInfo, StorageError>;
}

#[async_trait::async_trait]
pub trait Session: Send {
    async fn begin(&mut self) -> Result<Box<dyn Tx>, StorageError>;
    async fn query(&mut self, surrealql: &str, params: &NamedArgs) -> Result<QueryResult, StorageError>;
    async fn query_one<T: serde::de::DeserializeOwned>(&mut self, surrealql: &str, params: &NamedArgs)
        -> Result<Option<T>, StorageError>;
}

#[async_trait::async_trait]
pub trait Tx: Send {
    async fn execute(&mut self, surrealql: &str, params: &NamedArgs) -> Result<QueryResult, StorageError>;
    async fn commit(self: Box<Self>) -> Result<(), StorageError>;
    async fn rollback(self: Box<Self>) -> Result<(), StorageError>;
}
```

**命名参数类型与规则**（见 §3）

```rust
pub type NamedArgs = std::collections::BTreeMap<String, serde_json::Value>;

pub struct QueryResult {
    pub rows: u64,
    pub bytes: u64,
    pub meta: serde_json::Value,
}

pub struct HealthInfo { pub ok: bool, pub message: String }
```

### 2.3 Repository（`spi/repo.rs`）

```rust
#[async_trait::async_trait]
pub trait Repository<T: Entity>: Send + Sync {
    async fn get(&self, tenant: &TenantId, id: &str) -> Result<Option<T>, StorageError>;
    async fn create(&self, tenant: &TenantId, doc: &T) -> Result<T, StorageError>;
    async fn upsert(&self, tenant: &TenantId, id: &str, patch: serde_json::Value, ver: Option<u64>)
        -> Result<T, StorageError>; // 乐观并发：WHERE ver = :ver
    async fn delete(&self, tenant: &TenantId, id: &str) -> Result<(), StorageError>;

    async fn select(
        &self,
        tenant: &TenantId,
        filter: serde_json::Value, // 简化条件树（适配器转换为 SurrealQL WHERE）
        sort: Option<Sort>, limit: u32, cursor: Option<String>
    ) -> Result<Page<T>, StorageError>;
}
```

### 2.4 图 / 检索 / 向量（`spi/graph.rs`, `search.rs`, `vector.rs`）

```rust
// Graph
#[async_trait::async_trait]
pub trait GraphStore: Send + Sync {
    async fn relate(&self, tenant: &TenantId, from: &str, edge: &str, to: &str, props: serde_json::Value) -> Result<(), StorageError>;
    async fn unrelate(&self, tenant: &TenantId, from: &str, edge: &str, to: &str) -> Result<(), StorageError>;
    async fn out<T: Entity>(&self, tenant: &TenantId, from: &str, edge: &str, limit: u32) -> Result<Vec<T>, StorageError>;
    async fn r#in<T: Entity>(&self, tenant: &TenantId, to: &str, edge: &str, limit: u32) -> Result<Vec<T>, StorageError>;
}

// Fulltext
#[async_trait::async_trait]
pub trait SearchStore: Send + Sync {
    async fn search<T: Entity>(&self, tenant: &TenantId, field: &str, q: &str, limit: u32) -> Result<Vec<T>, StorageError>;
}

// Vector (HNSW)
#[derive(Clone, Debug)]
pub struct VectorSpec { pub dim: u32, pub metric: String /* "cosine"|"l2" */, pub ef: Option<u32>, pub m: Option<u32> }

#[async_trait::async_trait]
pub trait VectorStore: Send + Sync {
    async fn upsert_vec(&self, tenant: &TenantId, id: &str, vec: &[f32]) -> Result<(), StorageError>;
    async fn knn<T: Entity>(&self, tenant: &TenantId, qvec: &[f32], k: u32, filter: Option<serde_json::Value>)
        -> Result<Vec<(T, f32)>, StorageError>;
}
```

### 2.5 迁移（`spi/migrate.rs`）

```rust
#[async_trait::async_trait]
pub trait Migrator: Send + Sync {
    async fn current_version(&self) -> Result<String, StorageError>;
    async fn apply_up(&self, scripts: &[MigrationScript]) -> Result<(), StorageError>;
    async fn apply_down(&self, scripts: &[MigrationScript]) -> Result<(), StorageError>;
}

pub struct MigrationScript {
    pub version: String,         // "2025-09-12T15-30-00__init"
    pub up_sql: String,          // SurrealQL
    pub down_sql: String,        // SurrealQL
    pub checksum: String,        // sha256
}
```

------

## 3. SurrealDB 适配层（`surreal/`）

### 3.1 连接与池（`config.rs`, `datastore.rs`）

```rust
pub struct SurrealConfig {
    pub endpoints: Vec<String>,  // ws://.. 或 http://..
    pub namespace: String,
    pub database: String,
    pub user: String,            // 或 root token
    pub pass: String,
    pub pool_min: u32,
    pub pool_max: u32,
    pub timeout_ms: u64,
}

pub struct SurrealDatastore { /* pool + cfg */ }

#[async_trait::async_trait]
impl Datastore for SurrealDatastore {
    async fn session(&self) -> Result<Box<dyn Session>, StorageError> { /* 从池取连接并选择 NS/DB */ }
    async fn health(&self) -> Result<HealthInfo, StorageError> { /* INFO 或 ping; 返回连接/索引状态摘要 */ }
}
```

**池化原则**

- 预热认证后放入池；空闲自动回收；连接断开自动重连。
- 不同 `NS/DB` 在配置时即固定；多库策略通过多实例管理。

### 3.2 命名参数规则与强约束（`binder.rs`）

- **命名参数统一使用 `$param` 形式**（SurrealQL 规范）。
- **强制租户条件**：
  - 所有查询自动检查/注入 `WHERE tenant = $tenant` 或 ID 前缀校验（`table:tenant_*`）。
  - `NamedArgs` **必须**包含 `"tenant" -> TenantId`；缺失即拒绝执行。
- **绑定示例**：

```rust
let sql = "SELECT * FROM user WHERE tenant = $tenant AND email = $email LIMIT $limit";
let mut args = NamedArgs::new();
args.insert("tenant".into(), json!(tenant.0));
args.insert("email".into(), json!(email));
args.insert("limit".into(), json!(20));
```

- **禁拼接**：拼接任何用户输入到 SurrealQL 字符串即视为错误（开发时 lint + 运行时守卫）。

### 3.3 Session/Tx 实现（`datastore.rs`, `tx.rs`）

- `Session.query`：包裹**计时/指标** → Surreal 客户端执行 → 解析结果 → 记录 `rows/bytes`。
- `Tx`：`BEGIN TRANSACTION; ... COMMIT;`，失败 `ROLLBACK`；提供**重试钩子**（指数退避）。

### 3.4 Repository 映射（`mapper.rs`）

- `get`

  ```sql
  SELECT * FROM type::thing($table, $id) WHERE tenant = $tenant;
  ```

- `create`（id 由上游生成）

  ```sql
  CREATE type::thing($table, $id) CONTENT $doc;
  ```

- `upsert`（乐观并发）

  ```sql
  UPDATE type::thing($table, $id) PATCH $patch WHERE ver = $ver AND tenant = $tenant RETURN AFTER;
  ```

- `select`（条件树 → SurrealQL WHERE + ORDER + START/LIMIT 游标）

### 3.5 Graph 映射

- `relate`

  ```sql
  RELATE $from -> $edge -> $to CONTENT { tenant: $tenant, ... };
  ```

- `out`

  ```sql
  SELECT out.* FROM $from -> $edge WHERE tenant = $tenant LIMIT $limit;
  ```

### 3.6 搜索与向量（`search.rs`, `vector.rs`）

- **全文**
  - 定义：`DEFINE INDEX idx_ft ON TABLE post FIELDS title, body SEARCH ANALYZER ...;`
  - 查询：`SELECT * FROM post WHERE tenant=$tenant AND SEARCH::SCORE(post, $q) > 0 ORDER BY ... LIMIT $k;`
- **向量（HNSW）**
  - 定义：`DEFINE INDEX idx_vec ON TABLE doc FIELDS embedding VECTOR(512) HNSW ...;`
  - Upsert：`UPDATE type::thing('doc', $id) SET embedding = $vec WHERE tenant = $tenant;`
  - kNN：`SELECT *, SIMILARITY::COSINE(embedding, $qvec) AS score FROM doc WHERE tenant=$tenant ORDER BY score DESC LIMIT $k;`

> 抽象层屏蔽具体函数名，通过 `VectorSpec.metric` 选择 `COSINE/L2` 等。

------

## 4. 迁移脚本规范（`surreal/migrate.rs`）

### 4.1 目录与命名

```
migrations/
  2025-09-12T15-30-00__init.up.surql
  2025-09-12T15-30-00__init.down.surql
  2025-09-18T10-05-00__add_vector_index.up.surql
  2025-09-18T10-05-00__add_vector_index.down.surql
```

- 文件内容为 **SurrealQL**；`up` 与 `down` 成对出现。
- 迁移系统计算 **sha256** 校验并记录。

### 4.2 迁移版本表

```sql
DEFINE TABLE _migrations SCHEMAFULL;
DEFINE FIELD version ON _migrations TYPE string ASSERT string::len($value) > 0;
DEFINE FIELD checksum ON _migrations TYPE string;
DEFINE FIELD applied_at ON _migrations TYPE datetime;
DEFINE INDEX uniq_ver ON TABLE _migrations COLUMNS version UNIQUE;
```

### 4.3 执行流程

1. 读取目录，按时间排序；
2. 查询 `_migrations` 获取已应用版本；
3. 对未应用的 **在事务中** 依次执行 `.up.surql`；写入 `_migrations`；
4. 回滚时按逆序执行 `.down.surql`；
5. 审计：生成 `Envelope<MigrationEvent>`（version/ok/error/elapsed）。

------

## 5. 错误模型与映射（`errors.rs`, `surreal/errors.rs`）

### 5.1 StorageError（平台稳定码）

```rust
#[derive(thiserror::Error, Debug)]
pub enum StorageError {
  #[error("{0}")]
  Obj(soulbase_errors::prelude::ErrorObj),
}

impl StorageError {
  pub fn provider_unavailable(msg: impl Into<String>) -> Self { ... }    // PROVIDER.UNAVAILABLE
  pub fn conflict(msg: impl Into<String>) -> Self { ... }                 // STORAGE.CONFLICT
  pub fn not_found(msg: impl Into<String>) -> Self { ... }                // STORAGE.NOT_FOUND
  pub fn schema(msg: impl Into<String>) -> Self { ... }                   // SCHEMA.VALIDATION_FAILED
  pub fn unknown(msg: impl Into<String>) -> Self { ... }                  // UNKNOWN.INTERNAL
}
```

### 5.2 适配层映射原则

- **连接/超时/网络** → `provider_unavailable`；
- **唯一约束/版本不匹配** → `conflict`；
- **无结果**（`get/select` 空）→ `not_found`（或返回 `Ok(None)`，视接口）；
- **解析/参数绑定错误** → `schema`；
- 其它 → `unknown`；
- 对外统一返回 `ErrorObj` 的公共视图（码与短消息），诊断细节仅入审计。

------

## 6. 指标出口（`observe.rs`, `surreal/observe.rs`）

### 6.1 计时与标签

- 每次 **Session.query/Tx.execute**：
  - 计时：`latency_ms`（总时长）；
  - 统计：`rows`, `bytes`;
  - 标签：`tenant`, `table`, `kind=read|write|tx|graph|search|vector`, `statement`, `index_hit`（可选）, `code`。

```rust
pub struct StorageLabels<'a> {
  pub tenant: &'a str,
  pub table: &'a str,
  pub kind: &'a str,
  pub code: Option<&'a str>,
}
```

### 6.2 预聚合指标（示例）

- `storage_requests_total{kind,table}`
- `storage_latency_ms_bucket{kind,table}`（直方图）
- `storage_rows_total{table}` / `storage_bytes_total{table}`
- `storage_errors_total{code}`
- `storage_tx_rollback_total`
- `storage_vector_qps{k}` / `storage_search_qps`

> 与 `soulbase-observe` 集成时，保持标签键一致性（`code/retryable/severity` 对齐 `soulbase-errors`）。

------

## 7. 租户与幂等

- **租户**：在 `NamedArgs` 强制存在 `$tenant`，并在 SurrealQL 注入 `WHERE tenant = $tenant`；ID 前缀双重校验（`table:tenant_*`）。
- **幂等**：在跨语句事务场景可选启用“幂等密钥”参数；由调用方传入 `idempotency_key`，适配层在 `_idempotency` 表记录最近提交摘要与时间窗口，命中则短路返回。

------

## 8. 性能与重试

- **读写**：提供指数退避重试器（仅对 `PROVIDER.UNAVAILABLE` 与可重试错误）；
- **事务**：冲突重试（上限 N 次，Jitter）+ 观测打点；
- **索引**：`INFO FOR INDEX` 周期采样；新建索引期间对关键读路径配置“降级查询”策略（例如放宽排序或减少 JOIN/RELATE 步数）。

------

## 9. 开发指南（要点）

- 一律**参数化**；禁止字符串拼接 SQL。
- 读操作优先走 **Repository.select** 的条件树；复杂场景用 **Session.query** 但仍需 `WHERE tenant = $tenant`。
- `Entity` 的 `TABLE` 常量与架构迁移脚本保持一致；更新字段时需同步迁移。
- 对外公开的 `Repository` 不返回 Surreal 原始类型，始终为业务实体或分页结构。
- 所有写入必须带 `Envelope` 的 `partition_key` 与时间戳字段，便于回放。

------

## 10. 示例（接口组合示意）

```rust
// 读一个用户
let mut sess = ds.session().await?;
let user: Option<User> = sess.query_one(
    "SELECT * FROM type::thing($table, $id) WHERE tenant=$tenant",
    &named!{ "table": "user", "id": uid, "tenant": tenant.0 }
).await?;

// 事务更新（乐观并发）
let mut tx = sess.begin().await?;
let res = tx.execute(
    "UPDATE type::thing($table, $id) PATCH $patch WHERE tenant=$tenant AND ver=$ver RETURN AFTER",
    &named!{ "table":"doc", "id": id, "tenant": tenant.0, "patch": patch, "ver": ver }
).await?;
tx.commit().await?;

// 向量检索
let hits: Vec<(Doc, f32)> = vector.knn(&tenant, &qvec, 10, None).await?;
```

> `named!{..}` 为便捷宏（RIS 中提供），确保 `$tenant` 存在。

------

## 11. 版本化与兼容

- SPI 遵循 **SemVer**；新增方法以默认实现保障向后兼容。
- Surreal 适配层针对 2.0–2.3 的差异提供 **能力检测**（向量/全文函数名差异、语法差异）与**降级路径**。
- 迁移脚本携带 `-- requires >=x.y.z` 注释，运行期校验服务端版本满足要求。

------

## 12. 开放问题

- LIVE 查询（订阅）作为 `events` 抽象是否纳入本模块或上移到 `soulbase-observe`；
- HNSW 在线重建期间的“影子索引 + 切换”机制是否沉淀为通用组件；
- 向量字段的压缩与持久化成本（PQ/量化）是否在存储层提供统一适配。

------

以上 TD 将在接下来的 **SB-09-RIS（最小可运行骨架）** 中落成：包含 SurrealDatastore 的连接/会话/查询占位实现、命名参数绑定守卫、Repository/Graph/Vector 的最小路径、迁移执行器骨架与单测（参数化/租户过滤/事务/错误映射）。
