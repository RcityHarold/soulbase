# 文档 SB-14-TD：`soulbase-qos` 技术设计

（Quotas · Costing · Retention · Gate）

> 对应规约：SB-14
>  目标：给出**可落地**的 QoS 技术方案与接口：**策略/价目加载、配额检查（预留/直扣）、结算记账、账页与对账、留存归档、节流与降级、观测与证据、幂等与热更**。与 `soulbase-*` 全家桶保持**稳定错误码/统一标签/最小披露**不变式。

------

## 1. Crate 结构与 Feature 规划

```
soulbase-qos/
  src/
    lib.rs
    errors.rs                 # QosError → soulbase-errors 稳定码
    model/                    # DTO：Policy/Price/Units/Usage/Reservation/Charge/Ledger/Retention
      mod.rs
      key.rs                  # BudgetKey/ResourceAction 规范化
      units.rs                # Unit / UsageEstimate / UsageActual
      policy.rs               # QuotaPolicy/Window/Limit/DegradePlan/Inheritance
      price.rs                # PricingKey/PricingTable/PriceRule/Stepper(阶梯价)
      reserve.rs              # ReservationHandle/Outcome 语义
      charge.rs               # Charge/Cost/Tax(预留)
      ledger.rs               # LedgerLine/LedgerPeriod
      retention.rs            # RetentionRule/Selector
    spi/                      # 抽象 SPI
      mod.rs
      policy_store.rs         # 读取/缓存策略（支持热更）
      price_store.rs          # 价目表
      limiter.rs              # 速率器/令牌桶/滑窗
      reservation.rs          # 预留存储（等幂/冲销）
      ledger_store.rs         # 账页存储/对账
      retention_exec.rs       # 留存执行器（归档/清理）
      reconcile.rs            # 对账接口（与 Provider/账单）
    alg/                      # 算法实现
      token_bucket.rs         # 令牌桶（突发 + 持续速率）
      sliding_window.rs       # 滑动窗口/滚动窗口
    facade.rs                 # QosFacade：统一入口（check/reserve/settle/throttle/degrade）
    observe.rs                # 指标与证据事件（对接 SB-11）
    surreal/                  # SurrealDB 落地（可选）
      schema.surql            # 表/索引 DDL（供迁移使用）
      repo.rs                 # Store 实现（基于 SB-09 Storage SPI）
    prelude.rs
```

**Features**

- `surreal`：启用 SurrealDB 适配（默认可关闭）
- `qps-only`：只启用轻量速率器（不计费/账页）
- `observe`：启用指标/证据导出
- `strict-retention`：留存执行器强制只跑在“安全窗口”
- `pricing-step`：阶梯价/区域价支持

------

## 2. 核心数据（`model/*`）

### 2.1 预算键与资源动作（`key.rs`）

```rust
#[derive(Clone, Debug, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub struct BudgetKey {
  pub tenant: String,                // 必填
  pub project: Option<String>,
  pub subject: Option<String>,       // user/service/agent id
  pub resource: String,              // e.g. "soul:model:gpt-4o" | "soul:tool:browser"
  pub action: String,                // invoke|read|write|…
}
```

> 规范：`resource` 与 `action` 对齐 `soulbase-auth`/`-tools`/`-llm` 的 URN 与动词；所有接口**强制**携带 `tenant`。

### 2.2 计量单位与用量（`units.rs`）

```rust
#[derive(Clone, Debug, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Unit { TokensIn, TokensOut, Calls, BytesIn, BytesOut, CpuMs, GpuMs, StorageGbDay, Objects, Retries }

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct UsageEstimate { pub map: std::collections::BTreeMap<Unit, u64> }   // 调用前估算
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct UsageActual   { pub map: std::collections::BTreeMap<Unit, u64> }   // 调用后实报
```

### 2.3 策略（`policy.rs`）

```rust
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Window { PerMin, PerHour, PerDay, PerMonth, Rolling(u64) } // Rolling(ms)

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Limit { pub soft: u64, pub hard: u64, pub burst: u64 }

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DegradePlan { pub model_fallback: Option<String>, pub disable_tools: bool, pub read_only: bool }

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct QuotaPolicy {
  pub key_prefix: BudgetKey,                    // 可以用 "*" 通配 project/subject
  pub window: Window,
  pub unit: Unit,                               // 限额针对某个单位
  pub limit: Limit,
  pub priority: String,                         // "interactive" | "background"
  pub degrade: Option<DegradePlan>,
  pub inherit: Option<String>,                  // 父策略 id（租户级 → 项目级 → 主体级）
  pub version_hash: String,                     // 便于证据与热更
}
```

### 2.4 价目与计费（`price.rs`）

```rust
#[derive(Clone, Debug, Hash, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PricingKey { pub provider: String, pub model: Option<String>, pub region: Option<String>, pub unit: Unit }

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PriceRule { pub per_unit_usd: f32, pub tier: Option<(u64, u64)> }  // [min,max] 阶梯

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PricingTable { pub version: String, pub rules: std::collections::BTreeMap<PricingKey, Vec<PriceRule>> }
```

### 2.5 预留/结算/账页（`reserve.rs` `charge.rs` `ledger.rs`）

```rust
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum QuotaOutcome { Allowed, RateLimited, BudgetExceeded }

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ReservationHandle { pub id: String, pub key: BudgetKey, pub version_hash: String, pub expires_at: i64 }

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Charge { pub unit: Unit, pub quantity: u64, pub unit_price: f32, pub amount_usd: f32, pub meta: serde_json::Value }

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct LedgerLine { pub tenant: String, pub envelope_id: String, pub period: String, pub charges: Vec<Charge>, pub total_usd: f32 }
```

### 2.6 留存（`retention.rs`）

```rust
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum RetentionClass { Hot, Warm, Cold, Frozen }

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Selector { pub kind: String, pub labels: std::collections::BTreeMap<String,String> } // e.g. kind="evidence", labels={"domain":"sandbox"}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RetentionRule { pub class: RetentionClass, pub ttl_days: u32, pub archive_to: Option<String>, pub selector: Selector, pub version_hash: String }
```

------

## 3. SPI 抽象（`spi/*`）

### 3.1 策略与价目加载

```rust
#[async_trait::async_trait]
pub trait PolicyStore: Send + Sync {
  async fn load(&self, tenant: &str) -> Result<Vec<QuotaPolicy>, QosError>;
  fn version(&self) -> String;                               // 全局策略版本哈希
}

#[async_trait::async_trait]
pub trait PriceStore: Send + Sync {
  async fn table(&self) -> Result<PricingTable, QosError>;
}
```

### 3.2 限速器（突发桶/滑窗）

```rust
#[async_trait::async_trait]
pub trait Limiter: Send + Sync {
  /// 试图消耗 amount 单位（可为估算值或 1 call），返回是否允许与待触发的降级建议
  async fn consume(&self, key: &BudgetKey, unit: Unit, amount: u64, window: &Window, limit: &Limit)
      -> Result<(QuotaOutcome, Option<DegradePlan>), QosError>;
}
```

### 3.3 预留存储与等幂

```rust
#[async_trait::async_trait]
pub trait ReservationStore: Send + Sync {
  async fn create(&self, env_id: &str, key: &BudgetKey, est: &UsageEstimate, version_hash: &str, ttl_ms: u64)
      -> Result<ReservationHandle, QosError>;
  async fn settle(&self, env_id: &str, handle_id: &str, actual: &UsageActual)
      -> Result<Vec<Charge>, QosError>;                 // 等幂：同一 env_id 重复 settle 返回相同 Charge
  async fn cancel(&self, env_id: &str, handle_id: &str) -> Result<(), QosError>;
}
```

### 3.4 账页/对账/留存

```rust
#[async_trait::async_trait]
pub trait LedgerStore: Send + Sync {
  async fn append(&self, line: LedgerLine) -> Result<(), QosError>;
  async fn sum_tenant(&self, tenant: &str, period: &str) -> Result<f32, QosError>;
}

#[async_trait::async_trait]
pub trait Reconciler: Send + Sync {
  async fn reconcile(&self, provider_report: serde_json::Value, local: Vec<LedgerLine>)
      -> Result<Vec<serde_json::Value>, QosError>;      // 差异列表
}

#[async_trait::async_trait]
pub trait RetentionExec: Send + Sync {
  async fn run(&self, rule: &RetentionRule) -> Result<u64, QosError>;  // 返回处理条数
}
```

------

## 4. 算法实现（`alg/*`）

### 4.1 令牌桶（突发 + 持续速率）

- 桶容量 = `burst`；补充速率 = `soft / window`；
- 消耗顺序：先桶、后软限（触发降级建议）、超出软限但 < 硬限→`RateLimited`；≥硬限→`BudgetExceeded`。
- 实现支持**分布式**：以 `(BudgetKey, Unit, Window)` 为键，将 `tokens, last_refill_at` 存 `ReservationStore` 或高性能 KV；使用 CAS 更新。

### 4.2 滑动窗口（Rolling）

- 将窗口拆分为 `N` 个固定片段（例如 5 x 12s = 1 min）；
- 每片段计数器求和；写入采用原子自增；
- 适合限制 `Calls`/`Retries` 等离散单位。

> 两者可组合：复杂策略优先用令牌桶，辅助以滑窗计总量。

------

## 5. 统一入口门面（`facade.rs`）

```rust
pub struct QosFacade<P: PolicyStore, R: ReservationStore, L: Limiter, C: PriceStore, G: LedgerStore> {
  pub policy: P, pub reserv: R, pub limiter: L, pub price: C, pub ledger: G,
}

impl<P,R,L,C,G> QosFacade<P,R,L,C,G>
where P:PolicyStore, R:ReservationStore, L:Limiter, C:PriceStore, G:LedgerStore {
  /// 直扣（无需预留）：用于简单“Calls/Bytes*”的快路径（与 Auth.Authorizer::check_and_consume 对齐）
  pub async fn check_and_consume(&self, key: &BudgetKey, unit: Unit, amount: u64)
      -> Result<QuotaOutcome, QosError> {
    let pols = self.policy.load(&key.tenant).await?;
    let pol = select_policy(&pols, key, unit);                 // 按继承/最匹配选择
    let (outcome, _deg) = self.limiter.consume(key, unit, amount, &pol.window, &pol.limit).await?;
    Ok(outcome)
  }

  /// 预留：返回可选降级建议与 handle
  pub async fn reserve(&self, env_id: &str, key: &BudgetKey, est: &UsageEstimate, ttl_ms: u64)
      -> Result<(QuotaOutcome, Option<DegradePlan>, Option<ReservationHandle>), QosError> {
    let pols = self.policy.load(&key.tenant).await?;
    // 逐 unit 试消费（估算值），若任何一个达到硬限 → 拒绝
    let mut suggest: Option<DegradePlan> = None;
    for (unit, qty) in &est.map {
      let pol = select_policy(&pols, key, *unit);
      let (o, d) = self.limiter.consume(key, *unit, *qty, &pol.window, &pol.limit).await?;
      match o {
        QuotaOutcome::Allowed => { if suggest.is_none(){ suggest = d } }
        QuotaOutcome::RateLimited => { suggest = suggest.or(d); }
        QuotaOutcome::BudgetExceeded => return Ok((QuotaOutcome::BudgetExceeded, d, None)),
      }
    }
    // 允许则创建预留 handle（记录版本哈希）
    let vh = self.policy.version();
    let handle = self.reserv.create(env_id, key, est, &vh, ttl_ms).await?;
    Ok((QuotaOutcome::Allowed, suggest, Some(handle)))
  }

  /// 结算：根据实际用量与价目计算 Charge；等幂
  pub async fn settle(&self, env_id: &str, handle: &ReservationHandle, actual: &UsageActual)
      -> Result<Vec<Charge>, QosError> {
    let table = self.price.table().await?;
    let charges = calc_charges(&table, actual);
    let lines = LedgerLine{
      tenant: handle.key.tenant.clone(),
      envelope_id: env_id.into(),
      period: current_period(), charges: charges.clone(),
      total_usd: charges.iter().map(|c| c.amount_usd).sum(),
    };
    self.ledger.append(lines).await?;
    let _ = emit_evidence_settle(&handle.key, &charges);        // SB-11
    Ok(charges)
  }
}
```

**辅助**：`select_policy()` 支持策略继承/覆盖：匹配顺序 **subject→project→tenant**，最精确优先；`calc_charges()` 根据 `PricingTable` 将各 `unit` 的 `quantity` 乘以对应单价（支持阶梯价/区域价）。

------

## 6. 错误映射与证据（`errors.rs` `observe.rs`）

- **错误码**
  - 策略/价目缺失 → `SCHEMA.VALIDATION_FAILED`（或新增 `QOS.POLICY_MISSING`）
  - 存储/连接失败 → `PROVIDER.UNAVAILABLE`
  - 结算幂等冲突 → `STORAGE.CONFLICT`
  - 预算超限 → 面向调用方返回 `QUOTA.BUDGET_EXCEEDED`/`QUOTA.RATE_LIMITED`（由 Auth 层或拦截器统一处理）
- **指标**（与 SB-11 对齐标签）
  - `qos_reserve_total{tenant,resource,action,outcome}`
  - `qos_settle_usd_total{tenant,resource}`、`qos_units_total{unit}`
  - `qos_throttle_total{tenant,resource}`、`qos_degrade_total{plan}`
  - `qos_retention_archived_total{class}` / `qos_retention_expired_total{class}`
- **证据事件（Envelope）**
  - `QosReserveEvent{ key, est, policy_ver, outcome, degrade? }`
  - `QosSettleEvent{ key, actual, charges[], policy_ver, price_ver }`
  - `RetentionEvent{ selector, class, count }`

------

## 7. SurrealDB 落地（`surreal/schema.surql` 概要）

```sql
-- 预留/结算等幂锚点
DEFINE TABLE qos_reserv SCHEMAFULL;
DEFINE FIELD id           ON qos_reserv TYPE string;      -- handle_id
DEFINE FIELD env_id       ON qos_reserv TYPE string;      -- Envelope id（等幂锚）
DEFINE FIELD tenant       ON qos_reserv TYPE string;
DEFINE FIELD key          ON qos_reserv TYPE object;      -- BudgetKey 摘要
DEFINE FIELD est          ON qos_reserv TYPE object;      -- UsageEstimate 摘要
DEFINE FIELD version_hash ON qos_reserv TYPE string;
DEFINE FIELD expires_at   ON qos_reserv TYPE datetime;
DEFINE INDEX uniq_env ON TABLE qos_reserv COLUMNS env_id UNIQUE;

DEFINE TABLE qos_ledger SCHEMAFULL;
DEFINE FIELD tenant    ON qos_ledger TYPE string;
DEFINE FIELD period    ON qos_ledger TYPE string;         -- "2025-09"
DEFINE FIELD envelope  ON qos_ledger TYPE string;
DEFINE FIELD charges   ON qos_ledger TYPE array;          -- Charge[]
DEFINE FIELD total_usd ON qos_ledger TYPE number;
DEFINE INDEX idx_led   ON TABLE qos_ledger COLUMNS tenant, period;

DEFINE TABLE qos_bucket SCHEMAFULL;                       -- 令牌桶/滑窗状态
DEFINE FIELD k         ON qos_bucket TYPE string;         -- hash(BudgetKey+unit+window)
DEFINE FIELD tokens    ON qos_bucket TYPE float;
DEFINE FIELD last_at   ON qos_bucket TYPE datetime;
DEFINE INDEX uniq_bkt  ON TABLE qos_bucket COLUMNS k UNIQUE;

DEFINE TABLE qos_retention SCHEMAFULL;
DEFINE FIELD selector  ON qos_retention TYPE object;
DEFINE FIELD class     ON qos_retention TYPE string;
DEFINE FIELD ttl_days  ON qos_retention TYPE int;
DEFINE FIELD ver       ON qos_retention TYPE string;
```

> 通过 `soulbase-storage` 的 Repository/Tx 执行，强制 `$tenant` 参数化与 ID 前缀双重校验。

------

## 8. 热更策略

- **Policy/Price/Retention** 来自 `soulbase-config`：
  - 使用 **双缓冲** 缓存；`version_hash` 变更时**原子替换**；
  - 已获得 `ReservationHandle` 的调用在 `settle()` 时使用**其 `version_hash`** 结算，避免前后不一致；
- **Limiter** 的桶状态不清空，只按新策略速率渐变（避免抖动）。

------

## 9. 安全与幂等

- **幂等锚点**：`env_id`（同一 Envelope 的结算不会重复扣费）；
- **重入保护**：`ReservationHandle.expires_at` 到期自动取消；
- **最小披露**：账页仅存单位/数量/金额/模型名，不含请求原文或隐私参数；
- **多租户隔离**：所有表带 `tenant` 并强制 where 过滤；`BudgetKey` 只允许访问本租户资源。

------

## 10. 典型调用时序（与各模块对接）

### 10.1 LLM（SB-07）

1. 内核根据路由决定模型与 `UsageEstimate{TokensIn=估算, TokensOut=估算}`；
2. `qos.reserve(env_id, key={tenant,subject,model}, est)` → `Allowed`/`RateLimited(+Degrade)`/`BudgetExceeded`；
3. 调用模型；得到 `UsageActual{TokensIn, TokensOut}`；
4. `qos.settle(env_id, handle, actual)` → `Charge[]`（记录 `price.version`）；
5. 观测：`llm_requests_total` + `qos_settle_usd_total`。

### 10.2 工具/沙箱（SB-08/SB-06）

1. 按 Manifest 估算 `BytesOut/BytesIn/Calls` 预留；
2. 运行期间根据桶状态限速（Net/FS）；
3. 结算 `Bytes* + Calls`，写账页；
4. 留存：按 `RetentionRule` 归档大体积制品/证据。

### 10.3 存储/事务（SB-09/SB-10）

- **存储**：按 `StorageGbDay`（容量）与 `Bytes*`（IO）定期结算；
- **事务**：对 `Retries` 计量，超限 → 降级/拒绝。

------

## 11. 报错与降级建议对接

- 向调用者返回 `QuotaOutcome` 与 `DegradePlan`；
- 由**上游策略**（内核/拦截器）决定是否应用：
  - 切换 `model_fallback`；
  - 禁用工具或改“只读路径”；
  - 提示**重试间隔**（结合 limiter 的 `next_after` 计算）。

------

## 12. 开放问题

- **信用 & 欠额** 全流程（账期/催缴/冻结）；
- **复杂阶梯价** 与**包年包量**（commitment）的抵扣算法；
- **跨区域价目与汇率** 管理；
- **实时异常**：在 observe 中加入**成本激增**检测的 tail-sampling 触发规则。

------

### 小结

本技术设计提供了 `soulbase-qos` 的**数据模型、SPI、算法、门面以及存储/观测/热更/幂等**的完整落地路径：

- **调用前许可**（令牌桶/滑窗 + 降级建议）
- **统一结算**（价目表 → Charge → Ledger）
- **证据与指标**（可回放/可治理）
- **留存归档**（成本/合规驱动）

若认可，下一步将按“三件套”输出 **SB-14-RIS（最小可运行骨架）**：内存策略/价目/令牌桶实现、文件账页存储、门面 `QosFacade` 可运行示例与 2–3 个单测（LLM 预留/结算、工具限速、留存规则执行）。
