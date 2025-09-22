### **文档 SB-16：soulbase-cache（统一缓存与请求合并 / Unified Cache & SingleFlight）**

#### **第一部分：功能逻辑规约**

------

#### **0. 目标与范围（Objectives & Scope）**

- **目标**：提供一个**可复用、可观测、可治理**的**统一缓存层**，在不破坏语义的前提下**降低时延与成本**、防止**雪崩/击穿/放大**，为 LLM/Tools/Sandbox/Storage/A2A/Config 等热点读与只读外呼提供通用加速能力：
  1. **本地 LRU + 分布式缓存（Redis 适配）** 的两级缓存；
  2. **SingleFlight 请求合并**（相同 key 并发只触发一次真实加载）；
  3. **负缓存**（错误/空结果短 TTL）与 **SWR（Stale-While-Revalidate）**；
  4. **一致键空间**（tenant/namespace/hash）与**TTL 抖动（jitter）**；
  5. **可观测/可治理**：命中率、合并率、提前过期、容量与驱逐指标；统一失效/失效订阅机制。
- **范围**：
  - 核心抽象：`Cache`、`SingleFlight`、`Codec`（序列化）、`Policy`（TTL/Admission/Negative）、`Invalidation`（主动/订阅）；
  - 两级实现：`local`（进程内 LRU）/`redis`（跨实例共享）；
  - 与 `soulbase-observe`（指标）、`soulbase-qos`（命中=0 成本）、`soulbase-interceptors`（注入 trace/tenant）对齐。
- **非目标**：不提供分布式一致性 KV；不做强一致写缓存（无 write-back）；不替代业务级别的状态同步与事务（交给 `soulbase-storage/tx`）。

------

#### **1. 功能定位（Functional Positioning）**

- **读路径“降延迟 + 降成本”第一线**：LLM Prompt 语义哈希、Tools 的 `net.http` 只读 GET、对端元信息（A2A）、热点查询（Storage）等。
- **防雪崩与防击穿**：SingleFlight 合并 + 负缓存 + TTL 抖动 + 容量/速率守门。
- **平台级治理**：统一键命名/序列化、统一指标、统一失效协议（订阅/广播）。

------

#### **2. 系统角色与地位（System Role & Position）**

- 层级：**基石层 / Soul-Base**（横切读路径）。
- 关系：
  - **SB-07 LLM**：Prompt→语义哈希→缓存；结构化输出可缓存；
  - **SB-08/06 Tools/Sandbox**：`net.http` 的只读请求 + SWR；
  - **SB-09 Storage**：热点查询本地 LRU；跨实例 Redis；订阅表变更做主动失效；
  - **SB-15 A2A**：`PeerMetadata/Keys` 缓存；
  - **SB-03 Config**：策略/价目/留存快照缓存（需版本感知）
  - **SB-11 Observe**：`cache_hit/miss/merge/evict` 指标；
  - **SB-14 QoS**：命中即“0 成本”，写账页=0。

------

#### **3. 核心概念与数据模型（Core Concepts & Data Model）**

- **CacheKey**：`{tenant}:{namespace}:{hash(payload)}`（强制 tenant 维度）。
- **Tier**：`local`（进程 LRU）→miss→`redis`（可选）→miss→`loader`（真实加载）。
- **Entry**：`{value, codec, ttl_ms, created_at, headers?}`（value 序列化；可附响应元数据，如 ETag/Last-Modified）。
- **Policy**：
  - **TTL**（正向/负向）+ **Jitter**（±10–20%）
  - **SWR**：过期后先返回旧值，再后台刷新；
  - **Admission**：仅当 `size < limit`、`latency > threshold` 或 `code==OK` 才纳入；
  - **Negative**：错误/空集合短 TTL，避免放大。
- **SingleFlight**：相同 Key 的并发请求只触发一次 loader，其它请求等待/共享。
- **Invalidation**：主动 `del(key)`、批量 `del_prefix(tenant, namespace)`、订阅模式（Storage 变更/消息广播）。

------

#### **4. 不变式（Invariants）**

1. **最小披露**：缓存的 value 按 `Codec` 序列化，**不缓存凭证/密钥**；
2. **租户一致**：所有 Key 必含 `tenant`，跨租户命中被拒绝；
3. **可观测**：每次查得出 `hit/miss/merge/tier/latency/size`；
4. **防雪崩**：TTL 加随机抖动；大 Key/慢 loader 必须 SingleFlight；
5. **优雅降级**：Redis 故障=仅使用本地 LRU；不影响主路径可用性；
6. **等幂**：同 `Envelope.envelope_id` 的 loader 不重复执行业务副作用；
7. **一致键规范**：键名、编码、压缩算法在全局统一（避免“同值多键”）。

------

#### **5. 能力与接口（Abilities & Abstract Interfaces）**

> 仅定义行为；具体 trait 在 TD/RIS 落地。

- **Cache（KV + TTL）**
  - `get<T>(key) -> Option<T>`、`set<T>(key, val, ttl_ms)`、`delete(key)`、`delete_prefix(tenant, ns)`；
  - `get_or_load<T>(key, policy, loader) -> T`（内部：local→redis→loader + SingleFlight）；
- **SingleFlight**
  - `do_once(key, ttl_ms, loader) -> T`；
- **Codec**
  - `serialize<T>/deserialize<T>`（JSON/CBOR；可选 Snappy/deflate 压缩）；
- **SWR**
  - 返回旧值并触发后台刷新；
- **Invalidation**
  - 主动/批量/订阅（对应 Storage 表变更或消息总线广播 `cache.invalidate`）；
- **Metrics/Trace**
  - `cache_hit_total{tier}`, `cache_miss_total`, `cache_merge_total`, `cache_evict_total`, `cache_value_bytes`, `cache_get_ms_bucket`。

------

#### **6. 指标与 SLO / 验收（Metrics, SLOs & Acceptance）**

- **SLO**：
  - `get()` 自身开销 p95 **≤ 0.2ms**（local）/ **≤ 2ms**（redis）；
  - SingleFlight 合并率在高并发热点下 **≥ 90%**；
  - 负缓存降低错误放大 **≥ 50%**；
  - Redis 故障时，业务可用性不降级（回退 local）。
- **验收**：
  - 基准：命中率/合并率/尾延；
  - 契约：键规范/租户校验/负缓存/TTL 抖动；
  - 混沌：Redis 断链/超时/慢查询时仍可工作（降级到 local）。

------

#### **7. 依赖与边界（Dependencies & Boundaries）**

- **上游**：调用方（LLM/Tools/Sandbox/Storage/A2A/Config）与 `soulbase-interceptors`（注入 trace/tenant）；
- **下游**：本地内存（LRU）与可选 Redis；
- **边界**：不缓存**强一致写**（交给 Storage/Tx）；不缓存敏感内容或可变权限视图（除非带**租户+主体维度**）。

------

#### **8. 风险与控制（Risks & Controls）**

- **键漂移**：不同模块同义键不同名 → **统一 Key 规范**与 helper；
- **脏读/权限泄露**：缓存跨主体复用 → Key 加入 `tenant`（必要时加入 `subject`/`roles-hash`）；
- **放大**：热点 miss 时上游雪崩 → SingleFlight + 负缓存 + SWR；
- **Redis 单点**：断链导致抖动 → 本地兜底 + 限速；
- **过期风暴**：大批 key 同时到期 → TTL 抖动 + 预热。

------

#### **9. 关键交互序列（Key Interaction Sequences）**

**9.1 `get_or_load`（带 SingleFlight）**

1. 构造 `key={tenant}:{ns}:{hash(payload)}` → 查 local；
2. miss → 查 redis；
3. miss → **注册 SingleFlight**，仅首个请求执行 `loader()`，其余 await；
4. loader 成功 → set local/redis（带 jitter）→ 返回；失败 → set 负缓存（短 TTL）并返回错误（或 `Option`）。

**9.2 SWR 模式**

1. local 命中但过期 → 立即返回旧值 + **后台刷新**（刷新成功则回填）；
2. 刷新失败 → 记录 `cache_swr_refresh_failed_total`，旧值 TTL 小幅延长（可配置）。

**9.3 主动失效（订阅）**

1. Storage 表变更 → 发布 `cache.invalidate {tenant, ns, keys?}` 到总线；
2. 本地/redis 收到后批量删除对应 key 或 ns。

------

#### **10. 开放问题（Open Issues / TODO）**

- **多版本值**：LLM 结构化输出在不同 `model_alias` 下是否需要**多版本同 key**？（建议将 `model_alias` 纳入 namespace/hash）；
- **压缩策略**：大对象（>32KB）是否默认压缩？何时对冷数据只存 redis，不占用 local？
- **一致性策略**：对于“读后立刻写”的稀有场景是否需要**短时绕缓存**标记？
- **跨数据中心**：redis 多集群与地理延迟如何处理？（建议 DC 内就近命中，跨 DC 只走 local）。

------

> 若你认可该“功能规约”，下一步我将输出 **SB-16-TD（技术设计）**：给出 `Cache/SingleFlight/Codec/Policy/Invalidation` 的 Trait、两级实现（本地 LRU/Redis）的接口与关键细节（TTL 抖动、SWR、负缓存、请求合并、观测指标），随后提供 **SB-16-RIS** 可运行骨架与 2–3 个命中率/合并率的端到端单测。
