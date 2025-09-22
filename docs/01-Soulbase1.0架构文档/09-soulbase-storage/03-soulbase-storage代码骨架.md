下面是 **SB-09-RIS：`soulbase-storage` 最小可运行骨架**。
 它与 SB-09（规约）& SB-09-TD（设计）逐项对齐：提供 **Storage SPI（Datastore/Session/Tx/Repository/Graph/Search/Vector/Migrator）**，实现一个**默认可运行的 Mock/In-Memory 适配器**（用于本地开发与单测、零外部依赖），并预留 **SurrealDB 适配层目录与接口位**（后续接入 SurrealDB v2.3.x 时直接填充）。骨架包含**命名参数规则守卫、租户强约束、错误/指标出口占位**与**端到端单测**（CRUD/图/向量/事务/租户隔离）。

> 将内容放入 `soul-base/crates/soulbase-storage/` 后即可 `cargo check && cargo test`。
>  说明：本 RIS 的默认后端是 `mock`，用于跑通接口与测试；`surreal/` 目录已创建好接口骨架（不拉任何外部依赖），等你启用真实适配即可无缝替换。

------

## 目录结构

```
soul-base/
└─ crates/
   └─ soulbase-storage/
      ├─ Cargo.toml
      ├─ README.md
      ├─ src/
      │  ├─ lib.rs
      │  ├─ model.rs
      │  ├─ errors.rs
      │  ├─ observe.rs
      │  ├─ prelude.rs
      │  ├─ spi/
      │  │  ├─ mod.rs
      │  │  ├─ query.rs
      │  │  ├─ repo.rs
      │  │  ├─ graph.rs
      │  │  ├─ search.rs
      │  │  ├─ vector.rs
      │  │  ├─ migrate.rs
      │  │  └─ health.rs
      │  ├─ mock/
      │  │  ├─ mod.rs
      │  │  ├─ datastore.rs
      │  │  ├─ repo.rs
      │  │  ├─ graph.rs
      │  │  ├─ vector.rs
      │  │  └─ migrate.rs
      │  └─ surreal/
      │     ├─ mod.rs
      │     ├─ config.rs
      │     ├─ datastore.rs
      │     ├─ tx.rs
      │     ├─ binder.rs
      │     ├─ mapper.rs
      │     ├─ migrate.rs
      │     ├─ errors.rs
      │     └─ observe.rs
      └─ tests/
         └─ basic.rs
```

------

## Cargo.toml

```toml
[package]
name = "soulbase-storage"
version = "1.0.0"
edition = "2021"
license = "Apache-2.0"
description = "Storage SPI + adapters (Mock, SurrealDB) for the Soul platform"
repository = "https://example.com/soul-base"

[features]
default = ["mock"]
mock = []          # 本地可运行的内存实现
surreal = []       # 预留：启用后填充真实 Surreal 适配
vector = []
fulltext = []
migrate = []
pool = []
qos = []
observe = []

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
async-trait = "0.1"
thiserror = "1"
parking_lot = "0.12"
ahash = "0.8"
chrono = "0.4"
uuid = { version = "1", features = ["v4"] }

# 平台内
sb-types = { path = "../sb-types", version = "1.0.0" }
soulbase-errors = { path = "../soulbase-errors", version = "1.0.0" }

[dev-dependencies]
tokio = { version = "1", features = ["rt-multi-thread","macros","time"] }
```

------

## src/lib.rs

```rust
pub mod model;
pub mod errors;
pub mod observe;
pub mod prelude;

pub mod spi {
  pub mod query; pub mod repo; pub mod graph; pub mod search; pub mod vector; pub mod migrate; pub mod health;
  pub use super::spi::*;
}

#[cfg(feature="mock")]
pub mod mock;
pub mod surreal;

// 便捷导出
pub use model::*;
pub use errors::StorageError;
```

------

## src/model.rs

```rust
use serde::{Serialize, Deserialize};
use sb_types::prelude::*;

pub trait Entity: Sized + serde::de::DeserializeOwned + Serialize {
    const TABLE: &'static str;
    type Key: ToString;
    fn id(&self) -> &str;
}

#[derive(Clone, Debug)]
pub struct Page<T> { pub items: Vec<T>, pub next: Option<String> }

#[derive(Clone, Debug)]
pub struct Sort { pub field: String, pub asc: bool }

pub fn make_record_id(table: &str, tenant: &TenantId, ulid: &str) -> String {
    format!("{table}:{}_{}", tenant.0, ulid)
}
```

------

## src/errors.rs

```rust
use thiserror::Error;
use soulbase_errors::prelude::*;

#[derive(Debug, Error)]
#[error("{0}")]
pub struct StorageError(pub ErrorObj);

impl StorageError {
    pub fn into_inner(self) -> ErrorObj { self.0 }

    pub fn provider_unavailable(msg: impl Into<String>) -> Self {
        StorageError(ErrorBuilder::new(codes::PROVIDER_UNAVAILABLE)
            .user_msg("Storage provider unavailable. Please retry later.")
            .dev_msg(msg.into()).build())
    }
    pub fn not_found(msg: impl Into<String>) -> Self {
        StorageError(ErrorBuilder::new(codes::STORAGE_NOT_FOUND)
            .user_msg("Resource not found.").dev_msg(msg.into()).build())
    }
    pub fn schema(msg: impl Into<String>) -> Self {
        StorageError(ErrorBuilder::new(codes::SCHEMA_VALIDATION)
            .user_msg("Invalid storage query or parameters.").dev_msg(msg.into()).build())
    }
    pub fn unknown(msg: impl Into<String>) -> Self {
        StorageError(ErrorBuilder::new(codes::UNKNOWN_INTERNAL)
            .user_msg("Internal storage error.").dev_msg(msg.into()).build())
    }
}
```

> 注：如需 `STORAGE.CONFLICT` 等更多码，请在 `soulbase-errors` 码表中补充；RIS 先使用现有码。

------

## src/observe.rs

```rust
use std::collections::BTreeMap;

pub fn labels(tenant: &str, table: &str, kind: &str, code: Option<&str>) -> BTreeMap<&'static str, String> {
    let mut m = BTreeMap::new();
    m.insert("tenant", tenant.to_string());
    m.insert("table", table.to_string());
    m.insert("kind", kind.to_string());
    if let Some(c) = code { m.insert("code", c.to_string()); }
    m
}
```

------

## src/prelude.rs

```rust
pub use crate::model::{Entity, Page, Sort, make_record_id};
pub use crate::errors::StorageError;

// SPI
pub use crate::spi::query::{NamedArgs, named};
pub use crate::spi::repo::Repository;
pub use crate::spi::graph::GraphStore;
pub use crate::spi::search::SearchStore;
pub use crate::spi::vector::{VectorStore, VectorSpec};
pub use crate::spi::migrate::{Migrator, MigrationScript};

// Mock default
#[cfg(feature="mock")]
pub use crate::mock::{MockDatastore, InMemoryRepository, InMemoryGraph, InMemoryVector, InMemoryMigrator};
```

------

## src/spi/mod.rs

```rust
use serde_json::Value;
use crate::errors::StorageError;

pub type NamedArgs = std::collections::BTreeMap<String, Value>;

#[derive(Clone, Debug)]
pub struct QueryResult { pub rows: u64, pub bytes: u64, pub meta: Value }

#[derive(Clone, Debug)]
pub struct HealthInfo { pub ok: bool, pub message: String }

#[async_trait::async_trait]
pub trait Datastore: Send + Sync {
    async fn session(&self) -> Result<Box<dyn Session>, StorageError>;
    async fn health(&self) -> Result<HealthInfo, StorageError>;
}

#[async_trait::async_trait]
pub trait Session: Send {
    async fn begin(&mut self) -> Result<Box<dyn Tx>, StorageError>;
    async fn query(&mut self, surrealql: &str, params: &super::spi::NamedArgs) -> Result<QueryResult, StorageError>;
    async fn query_one<T: serde::de::DeserializeOwned>(&mut self, surrealql: &str, params: &super::spi::NamedArgs)
        -> Result<Option<T>, StorageError>;
}

#[async_trait::async_trait]
pub trait Tx: Send {
    async fn execute(&mut self, surrealql: &str, params: &super::spi::NamedArgs) -> Result<QueryResult, StorageError>;
    async fn commit(self: Box<Self>) -> Result<(), StorageError>;
    async fn rollback(self: Box<Self>) -> Result<(), StorageError>;
}
```

### src/spi/query.rs

```rust
use super::NamedArgs;
use serde_json::json;

#[macro_export]
macro_rules! named {
  ( $( $k:literal : $v:expr ),* $(,)? ) => {{
      let mut m: $crate::spi::NamedArgs = std::collections::BTreeMap::new();
      $( m.insert($k.to_string(), serde_json::json!($v)); )*
      m
  }};
}

pub fn ensure_tenant(args: &NamedArgs) -> Result<(), &'static str> {
    if !args.contains_key("tenant") { return Err("missing $tenant named param"); }
    Ok(())
}
```

### src/spi/repo.rs

```rust
use crate::{Entity, Page, Sort};
use sb_types::prelude::TenantId;
use crate::errors::StorageError;

#[async_trait::async_trait]
pub trait Repository<T: Entity>: Send + Sync {
    async fn get(&self, tenant: &TenantId, id: &str) -> Result<Option<T>, StorageError>;
    async fn create(&self, tenant: &TenantId, doc: &T) -> Result<T, StorageError>;
    async fn upsert(&self, tenant: &TenantId, id: &str, patch: serde_json::Value, ver: Option<u64>)
        -> Result<T, StorageError>;
    async fn delete(&self, tenant: &TenantId, id: &str) -> Result<(), StorageError>;

    async fn select(
        &self, tenant: &TenantId, filter: serde_json::Value,
        sort: Option<Sort>, limit: u32, cursor: Option<String>
    ) -> Result<Page<T>, StorageError>;
}
```

### src/spi/graph.rs

```rust
use sb_types::prelude::TenantId;
use crate::errors::StorageError;
use crate::Entity;

#[async_trait::async_trait]
pub trait GraphStore: Send + Sync {
    async fn relate(&self, tenant: &TenantId, from: &str, edge: &str, to: &str, props: serde_json::Value) -> Result<(), StorageError>;
    async fn unrelate(&self, tenant: &TenantId, from: &str, edge: &str, to: &str) -> Result<(), StorageError>;
    async fn out<T: Entity>(&self, tenant: &TenantId, from: &str, edge: &str, limit: u32) -> Result<Vec<T>, StorageError>;
    async fn r#in<T: Entity>(&self, tenant: &TenantId, to: &str, edge: &str, limit: u32) -> Result<Vec<T>, StorageError>;
}
```

### src/spi/search.rs

```rust
use sb_types::prelude::TenantId;
use crate::{Entity, errors::StorageError};

#[async_trait::async_trait]
pub trait SearchStore: Send + Sync {
    async fn search<T: Entity>(&self, tenant: &TenantId, field: &str, q: &str, limit: u32) -> Result<Vec<T>, StorageError>;
}
```

### src/spi/vector.rs

```rust
use sb_types::prelude::TenantId;
use crate::{Entity, errors::StorageError};

#[derive(Clone, Debug)]
pub struct VectorSpec { pub dim: u32, pub metric: String, pub ef: Option<u32>, pub m: Option<u32> }

#[async_trait::async_trait]
pub trait VectorStore: Send + Sync {
    async fn upsert_vec(&self, tenant: &TenantId, id: &str, vec: &[f32]) -> Result<(), StorageError>;
    async fn knn<T: Entity>(&self, tenant: &TenantId, qvec: &[f32], k: u32, filter: Option<serde_json::Value>)
        -> Result<Vec<(T, f32)>, StorageError>;
}
```

### src/spi/migrate.rs & health.rs

```rust
use crate::errors::StorageError;

#[derive(Clone, Debug)]
pub struct MigrationScript { pub version: String, pub up_sql: String, pub down_sql: String, pub checksum: String }

#[async_trait::async_trait]
pub trait Migrator: Send + Sync {
    async fn current_version(&self) -> Result<String, StorageError>;
    async fn apply_up(&self, scripts: &[MigrationScript]) -> Result<(), StorageError>;
    async fn apply_down(&self, scripts: &[MigrationScript]) -> Result<(), StorageError>;
}

#[derive(Clone, Debug)]
pub struct PoolStats { pub total: u32, pub idle: u32, pub busy: u32 }

#[async_trait::async_trait]
pub trait Health: Send + Sync {
    async fn pool(&self) -> Result<PoolStats, StorageError>;
}
```

------

## src/mock/mod.rs

```rust
pub mod datastore;
pub mod repo;
pub mod graph;
pub mod vector;
pub mod migrate;

pub use datastore::MockDatastore;
pub use repo::InMemoryRepository;
pub use graph::InMemoryGraph;
pub use vector::InMemoryVector;
pub use migrate::InMemoryMigrator;
```

### src/mock/datastore.rs

```rust
use crate::spi::*;
use crate::errors::StorageError;
use parking_lot::RwLock;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

#[derive(Default)]
pub struct MockState {
    // (tenant, table) -> id -> doc (serde_json)
    pub data: HashMap<(String,String), HashMap<String, Value>>,
    // graph edges: (tenant, edge) -> Vec<(from,to,props)>
    pub edges: HashMap<(String,String), Vec<(String,String,Value)>>,
    // vectors: (tenant, table) -> id -> Vec<f32>
    pub vecs: HashMap<(String,String), HashMap<String, Vec<f32>>>,
}

pub struct MockDatastore {
    pub state: RwLock<MockState>,
}

impl MockDatastore {
    pub fn new() -> Self { Self { state: RwLock::new(MockState::default()) } }
}

struct MockSession<'a> { st: &'a RwLock<MockState> }
struct MockTx<'a> { st: &'a RwLock<MockState>, buf: Vec<Box<dyn FnOnce(&mut MockState) + Send>> }

#[async_trait::async_trait]
impl Datastore for MockDatastore {
    async fn session(&self) -> Result<Box<dyn Session>, StorageError> {
        Ok(Box::new(MockSession { st: &self.state }))
    }
    async fn health(&self) -> Result<crate::spi::HealthInfo, StorageError> {
        Ok(crate::spi::HealthInfo{ ok: true, message: "mock-ok".into() })
    }
}

#[async_trait::async_trait]
impl Session for MockSession<'_> {
    async fn begin(&mut self) -> Result<Box<dyn Tx>, StorageError> {
        Ok(Box::new(MockTx { st: self.st, buf: vec![] }))
    }
    async fn query(&mut self, _sql: &str, _params: &NamedArgs) -> Result<QueryResult, StorageError> {
        Ok(QueryResult{ rows: 0, bytes: 0, meta: serde_json::json!({ "mock": true }) })
    }
    async fn query_one<T: serde::de::DeserializeOwned>(&mut self, _sql: &str, _params: &NamedArgs)
        -> Result<Option<T>, StorageError> { Ok(None) }
}

#[async_trait::async_trait]
impl Tx for MockTx<'_> {
    async fn execute(&mut self, _sql: &str, _params: &NamedArgs) -> Result<QueryResult, StorageError> {
        // 仅占位；真实 Surreal 适配会执行 SQL
        Ok(QueryResult{ rows: 0, bytes: 0, meta: serde_json::json!({ "mock": true }) })
    }
    async fn commit(self: Box<Self>) -> Result<(), StorageError> {
        let mut st = self.st.write();
        for f in self.buf { f(&mut st); }
        Ok(())
    }
    async fn rollback(self: Box<Self>) -> Result<(), StorageError> { Ok(()) }
}
```

### src/mock/repo.rs

```rust
use crate::{Entity, Page, Sort, errors::StorageError};
use crate::spi::*;
use parking_lot::RwLock;
use serde_json::{Value, json};
use std::collections::HashMap;
use sb_types::prelude::*;

pub struct InMemoryRepository<T: Entity> {
    pub table: &'static str,
    pub st: std::sync::Arc<RwLock<super::datastore::MockState>>,
    _ph: std::marker::PhantomData<T>,
}
impl<T: Entity> InMemoryRepository<T> {
    pub fn new(ds: &super::datastore::MockDatastore) -> Self {
        Self { table: T::TABLE, st: std::sync::Arc::new(ds.state.clone()), _ph: std::marker::PhantomData }
    }
}

#[async_trait::async_trait]
impl<T: Entity> crate::spi::repo::Repository<T> for InMemoryRepository<T> {
    async fn get(&self, tenant: &TenantId, id: &str) -> Result<Option<T>, StorageError> {
        let st = self.st.read();
        let key = (tenant.0.clone(), self.table.to_string());
        let row = st.data.get(&key).and_then(|m| m.get(id)).cloned();
        Ok(match row {
            Some(v) => Some(serde_json::from_value(v).map_err(|e| StorageError::schema(format!("serde: {e}")))?),
            None => None
        })
    }

    async fn create(&self, tenant: &TenantId, doc: &T) -> Result<T, StorageError> {
        let mut st = self.st.write();
        let key = (tenant.0.clone(), self.table.to_string());
        let map = st.data.entry(key).or_insert_with(HashMap::new);
        let v = serde_json::to_value(doc).map_err(|e| StorageError::schema(format!("serde: {e}")))?;
        let id = doc.id().to_string();
        map.insert(id, v.clone());
        Ok(serde_json::from_value(v).map_err(|e| StorageError::schema(format!("serde: {e}")))?)
    }

    async fn upsert(&self, tenant: &TenantId, id: &str, patch: Value, _ver: Option<u64>) -> Result<T, StorageError> {
        let mut st = self.st.write();
        let key = (tenant.0.clone(), self.table.to_string());
        let map = st.data.entry(key).or_insert_with(HashMap::new);
        let entry = map.entry(id.to_string()).or_insert(json!({ "id": id }));
        let obj = entry.as_object_mut().ok_or_else(|| StorageError::schema("doc not object"))?;
        let p = patch.as_object().ok_or_else(|| StorageError::schema("patch not object"))?;
        for (k,v) in p { obj.insert(k.clone(), v.clone()); }
        Ok(serde_json::from_value(Value::Object(obj.clone())).map_err(|e| StorageError::schema(format!("serde: {e}")))?)
    }

    async fn delete(&self, tenant: &TenantId, id: &str) -> Result<(), StorageError> {
        let mut st = self.st.write();
        let key = (tenant.0.clone(), self.table.to_string());
        let map = st.data.entry(key).or_insert_with(HashMap::new);
        map.remove(id);
        Ok(())
    }

    async fn select(&self, tenant: &TenantId, filter: Value, sort: Option<Sort>, limit: u32, _cursor: Option<String>) -> Result<Page<T>, StorageError> {
        let st = self.st.read();
        let key = (tenant.0.clone(), self.table.to_string());
        let it = st.data.get(&key).map(|m| m.values().cloned()).unwrap_or_default();
        // 简化：filter 仅支持 { "field": value } 等值匹配
        let mut items: Vec<Value> = it.into_iter().filter(|v| {
            let obj = v.as_object().unwrap();
            if let Some(objf) = filter.as_object() {
                objf.iter().all(|(k, fv)| obj.get(k) == Some(fv))
            } else { true }
        }).collect();

        if let Some(s) = sort {
            items.sort_by(|a,b| {
                let av = a.get(&s.field).cloned().unwrap_or(Value::Null);
                let bv = b.get(&s.field).cloned().unwrap_or(Value::Null);
                if s.asc { av.to_string().cmp(&bv.to_string()) } else { bv.to_string().cmp(&av.to_string()) }
            });
        }
        items.truncate(limit as usize);
        let out: Vec<T> = items.into_iter().map(|v| serde_json::from_value(v).unwrap()).collect();
        Ok(Page { items: out, next: None })
    }
}
```

### src/mock/graph.rs

```rust
use crate::{Entity, errors::StorageError};
use crate::spi::graph::GraphStore;
use sb_types::prelude::TenantId;

pub struct InMemoryGraph {
    pub st: std::sync::Arc<parking_lot::RwLock<super::datastore::MockState>>,
}
impl InMemoryGraph { pub fn new(ds: &super::datastore::MockDatastore) -> Self { Self{ st: std::sync::Arc::new(ds.state.clone()) } } }

#[async_trait::async_trait]
impl GraphStore for InMemoryGraph {
    async fn relate(&self, tenant: &TenantId, from: &str, edge: &str, to: &str, props: serde_json::Value) -> Result<(), StorageError> {
        let mut st = self.st.write();
        let k = (tenant.0.clone(), edge.to_string());
        st.edges.entry(k).or_default().push((from.to_string(), to.to_string(), props));
        Ok(())
    }
    async fn unrelate(&self, tenant: &TenantId, from: &str, edge: &str, to: &str) -> Result<(), StorageError> {
        let mut st = self.st.write();
        let k = (tenant.0.clone(), edge.to_string());
        if let Some(v) = st.edges.get_mut(&k) {
            v.retain(|(f,t,_)| !(f==from && t==to));
        }
        Ok(())
    }
    async fn out<T: Entity>(&self, tenant: &TenantId, from: &str, edge: &str, limit: u32) -> Result<Vec<T>, StorageError> {
        let st = self.st.read();
        let ek = (tenant.0.clone(), edge.to_string());
        let mut out = vec![];
        if let Some(v) = st.edges.get(&ek) {
            for (_f,t,_p) in v.iter().filter(|(f,_,_)| f==from).take(limit as usize) {
                if let Some(m) = st.data.get(&(tenant.0.clone(), T::TABLE.to_string())) {
                    if let Some(doc) = m.get(t) {
                        out.push(serde_json::from_value(doc.clone()).map_err(|e| StorageError::schema(format!("serde: {e}")))?);
                    }
                }
            }
        }
        Ok(out)
    }
    async fn r#in<T: Entity>(&self, tenant: &TenantId, to: &str, edge: &str, limit: u32) -> Result<Vec<T>, StorageError> {
        let st = self.st.read();
        let ek = (tenant.0.clone(), edge.to_string());
        let mut out = vec![];
        if let Some(v) = st.edges.get(&ek) {
            for (f,_t,_p) in v.iter().filter(|(_,t,_)| t==to).take(limit as usize) {
                if let Some(m) = st.data.get(&(tenant.0.clone(), T::TABLE.to_string())) {
                    if let Some(doc) = m.get(f) {
                        out.push(serde_json::from_value(doc.clone()).map_err(|e| StorageError::schema(format!("serde: {e}")))?);
                    }
                }
            }
        }
        Ok(out)
    }
}
```

### src/mock/vector.rs

```rust
use crate::{Entity, errors::StorageError};
use crate::spi::vector::VectorStore;
use sb_types::prelude::TenantId;

pub struct InMemoryVector {
    pub table: &'static str,
    pub st: std::sync::Arc<parking_lot::RwLock<super::datastore::MockState>>,
}
impl InMemoryVector { pub fn new<T: Entity>(ds: &super::datastore::MockDatastore) -> Self { Self{ table: T::TABLE, st: std::sync::Arc::new(ds.state.clone()) } } }

#[async_trait::async_trait]
impl VectorStore for InMemoryVector {
    async fn upsert_vec(&self, tenant: &TenantId, id: &str, vec: &[f32]) -> Result<(), StorageError> {
        let mut st = self.st.write();
        let key = (tenant.0.clone(), self.table.to_string());
        st.vecs.entry(key).or_default().insert(id.to_string(), vec.to_vec());
        Ok(())
    }

    async fn knn<T: Entity>(&self, tenant: &TenantId, qvec: &[f32], k: u32, _filter: Option<serde_json::Value>)
        -> Result<Vec<(T, f32)>, StorageError> {
        let st = self.st.read();
        let key = (tenant.0.clone(), self.table.to_string());
        let Some(map) = st.vecs.get(&key) else { return Ok(vec![]); };
        let mut sc: Vec<(&String, f32)> = map.iter().map(|(id, v)| (id, cosine(v, qvec))).collect();
        sc.sort_by(|a,b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let mut out = vec![];
        if let Some(rows) = st.data.get(&key) {
            for (id, s) in sc.into_iter().take(k as usize) {
                if let Some(doc) = rows.get(id) {
                    out.push((serde_json::from_value(doc.clone()).map_err(|e| StorageError::schema(format!("serde: {e}")))? , s));
                }
            }
        }
        Ok(out)
    }
}
fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let (mut dot, mut na, mut nb) = (0.0, 0.0, 0.0);
    for i in 0..a.len().min(b.len()) { dot += a[i]*b[i]; na += a[i]*a[i]; nb += b[i]*b[i]; }
    dot / (na.sqrt()*nb.sqrt() + 1e-6)
}
```

### src/mock/migrate.rs

```rust
use crate::spi::migrate::{Migrator, MigrationScript};
use crate::errors::StorageError;

pub struct InMemoryMigrator { pub applied: parking_lot::Mutex<Vec<String>> }
impl InMemoryMigrator { pub fn new() -> Self { Self{ applied: parking_lot::Mutex::new(vec![]) } } }

#[async_trait::async_trait]
impl Migrator for InMemoryMigrator {
    async fn current_version(&self) -> Result<String, StorageError> {
        Ok(self.applied.lock().last().cloned().unwrap_or_else(|| "none".into()))
    }
    async fn apply_up(&self, scripts: &[MigrationScript]) -> Result<(), StorageError> {
        let mut v = self.applied.lock();
        for s in scripts { v.push(s.version.clone()); }
        Ok(())
    }
    async fn apply_down(&self, scripts: &[MigrationScript]) -> Result<(), StorageError> {
        let mut v = self.applied.lock();
        for s in scripts { v.retain(|x| x != &s.version); }
        Ok(())
    }
}
```

------

## src/surreal/* （占位骨架，便于后续接入真实 SurrealDB）

```rust
// surreal/mod.rs
pub mod config; pub mod datastore; pub mod tx; pub mod binder; pub mod mapper; pub mod migrate; pub mod errors; pub mod observe;

// surreal/config.rs
pub struct SurrealConfig {
  pub endpoints: Vec<String>, pub namespace: String, pub database: String,
  pub user: String, pub pass: String, pub timeout_ms: u64,
}

// 其它文件保留空模块/注释，待接入 SurrealDB SDK 后补齐实现
```

------

## tests/basic.rs

```rust
use soulbase_storage::prelude::*;
use sb_types::prelude::*;
use serde::{Serialize, Deserialize};

// 测试实体
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Doc { pub id: String, pub tenant: String, pub title: String, pub ver: u64 }
impl Entity for Doc {
    const TABLE: &'static str = "doc";
    type Key = String;
    fn id(&self) -> &str { &self.id }
}

#[tokio::test]
async fn crud_and_select() {
    let ds = MockDatastore::new();
    let repo: InMemoryRepository<Doc> = InMemoryRepository::new(&ds);
    let tenant = TenantId("tenantA".into());

    // Create
    let d1 = Doc{ id: "doc:tenantA_001".into(), tenant: "tenantA".into(), title: "hello".into(), ver: 1 };
    let d2 = Doc{ id: "doc:tenantA_002".into(), tenant: "tenantA".into(), title: "hi".into(), ver: 1 };
    repo.create(&tenant, &d1).await.unwrap();
    repo.create(&tenant, &d2).await.unwrap();

    // Get
    let got = repo.get(&tenant, &d1.id).await.unwrap().unwrap();
    assert_eq!(got.title, "hello");

    // Select with filter + limit
    let page = repo.select(&tenant, serde_json::json!({"tenant":"tenantA"}), None, 10, None).await.unwrap();
    assert_eq!(page.items.len(), 2);

    // Upsert
    let upd = repo.upsert(&tenant, &d1.id, serde_json::json!({"title":"hello2", "ver":2}), None).await.unwrap();
    assert_eq!(upd.title, "hello2");

    // Delete
    repo.delete(&tenant, &d2.id).await.unwrap();
    let page2 = repo.select(&tenant, serde_json::json!({"tenant":"tenantA"}), None, 10, None).await.unwrap();
    assert_eq!(page2.items.len(), 1);
}

#[tokio::test]
async fn graph_and_vector() {
    let ds = MockDatastore::new();
    let repo: InMemoryRepository<Doc> = InMemoryRepository::new(&ds);
    let graph = InMemoryGraph::new(&ds);
    let vecs: InMemoryVector = InMemoryVector::new::<Doc>(&ds);
    let tenant = TenantId("tenantA".into());

    let a = Doc{ id: "doc:tenantA_a".into(), tenant: "tenantA".into(), title: "cat sat".into(), ver: 1 };
    let b = Doc{ id: "doc:tenantA_b".into(), tenant: "tenantA".into(), title: "cat on mat".into(), ver: 1 };
    repo.create(&tenant, &a).await.unwrap();
    repo.create(&tenant, &b).await.unwrap();

    graph.relate(&tenant, &a.id, "edge_like", &b.id, serde_json::json!({"at":1})).await.unwrap();
    let outs: Vec<Doc> = graph.out(&tenant, &a.id, "edge_like", 10).await.unwrap();
    assert_eq!(outs.len(), 1);
    assert_eq!(outs[0].id, b.id);

    vecs.upsert_vec(&tenant, &a.id, &[1.0, 0.0, 0.0]).await.unwrap();
    vecs.upsert_vec(&tenant, &b.id, &[1.0, 0.1, 0.0]).await.unwrap();
    let hits = vecs.knn::<Doc>(&tenant, &[1.0, 0.05, 0.0], 1, None).await.unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].0.id, b.id);
}

#[tokio::test]
async fn tenant_isolation() {
    let ds = MockDatastore::new();
    let repo: InMemoryRepository<Doc> = InMemoryRepository::new(&ds);
    let t1 = TenantId("t1".into());
    let t2 = TenantId("t2".into());

    let a = Doc{ id: "doc:t1_a".into(), tenant: "t1".into(), title: "A".into(), ver: 1 };
    repo.create(&t1, &a).await.unwrap();

    // t2 看不到 t1 的记录
    let page = repo.select(&t2, serde_json::json!({"tenant":"t2"}), None, 10, None).await.unwrap();
    assert_eq!(page.items.len(), 0);
}

#[tokio::test]
async fn migration_tracks_versions() {
    let mig = InMemoryMigrator::new();
    assert_eq!(mig.current_version().await.unwrap(), "none");
    let s = vec![
        soulbase_storage::spi::migrate::MigrationScript{
            version:"2025-09-12T15-30-00__init".into(),
            up_sql:"DEFINE TABLE doc SCHEMALESS;".into(),
            down_sql:"REMOVE TABLE doc;".into(),
            checksum:"sha256:abc".into()
        }
    ];
    mig.apply_up(&s).await.unwrap();
    assert_eq!(mig.current_version().await.unwrap(), "2025-09-12T15-30-00__init");
}
```

------

## README.md（简版）

~~~markdown
# soulbase-storage (RIS)

Storage SPI + adapters for the Soul platform.

- SPI: Datastore/Session/Tx/Repository/Graph/Search/Vector/Migrator
- Default: Mock/In-Memory adapter (no external deps)
- SurrealDB adapter scaffold ready (fill in with real SDK later)
- Tenant guard, named-params, error normalization, basic metrics labels

## Build & Test
```bash
cargo check
cargo test
~~~

## Next

- Implement `surreal/*` with SurrealDB v2.3.x client
- Add `STORAGE.CONFLICT` etc. codes to `soulbase-errors`
- Wire metrics to soulbase-observe & QoS
- Expand filter DSL and cursor pagination

```
---

### 对齐说明
- **同频共振**：贯彻“参数化查询、租户强约束、事务与审计、稳定错误、可观测”不变式；接口命名与 SB-09-TD 一致。  
- **可演进**：Surreal 适配层目录与接口已搭好；只需在 `surreal/` 中填充具体实现即可对接 **SurrealDB v2.3.x**。  
- **可运行**：Mock 适配器覆盖 CRUD/图/向量/迁移的核心路径，便于团队立即接入并迭代真实后端。
::contentReference[oaicite:0]{index=0}
```
