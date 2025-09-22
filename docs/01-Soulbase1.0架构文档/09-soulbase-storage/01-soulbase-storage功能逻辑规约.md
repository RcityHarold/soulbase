### **文档 SB-09：soulbase-storage（存储抽象 + 适配 / SurrealDB）**

#### **第一部分：功能逻辑规约**

------

#### **0. 目标与范围（Objectives & Scope）**

- **目标**：为 Soul 生态提供**供应商无关**的存储抽象与“**SurrealDB 优先**”适配，实现统一的**数据读写、图关系（RELATE）、事务（BEGIN/COMMIT）、索引/向量索引、全文检索、迁移与健康可观测**等能力；同时保证与既有基座（types/auth/interceptors/errors/qos/observe）**口径一致**。
- **范围**：
  1. Storage SPI（Datastore / Tx / Repository / Graph / Migration / Health）；
  2. SurrealDB 适配：连接管理、参数化 SurrealQL、命名规范、NS/DB/多租户策略、索引与向量检索、事务；
  3. SLA/SLO、错误稳定码映射、审计与指标。
- **版本基线**：面向 **SurrealDB v2.3.x 稳定版（当前官网标注稳定为 v2.3.8）**，Rust SDK 覆盖 2.0.0–2.3.8 版本区间（以兼容矩阵保证） 。([SurrealDB](https://surrealdb.com/docs/surrealdb/installation?utm_source=chatgpt.com))

> 说明：SurrealDB 提供 SurrealQL、事务（`BEGIN ... COMMIT`）、索引与唯一约束、（自 v1.5 起）HNSW 向量索引、全文检索等能力，作为本模块的主要承载面。([SurrealDB](https://surrealdb.com/docs/surrealql?utm_source=chatgpt.com))

------

#### **1. 功能定位（Functional Positioning）**

- **SSoT 数据访问层**：对上提供统一的 Repository / Graph API；对下封装 SurrealDB（默认）与未来可插拔背端。
- **安全与一致性网关**：所有查询统一走**参数化**与**Schema 校验**路径；事务与幂等由本层归口。
- **观测闭环**：把请求-响应-错误-延迟-影响行数/向量命中等指标标准化输出到 `soulbase-observe`。

------

#### **2. 系统角色与地位（System Role & Position）**

- 所处层级：**基石层 / Soul-Base**（对上服务/内核/工具；对下数据库）。
- 关系：
  - `sb-types`：统一 `Id/Tenant/Envelope/PartitionKey`；
  - `soulbase-auth`：面向 Repository 的写路径需通过拦截器完成鉴权；
  - `soulbase-interceptors`：为入站请求注入 `TraceId/Envelope`；
  - `soulbase-errors`：统一映射 `STORAGE.* | PROVIDER.UNAVAILABLE | SCHEMA.VALIDATION_FAILED`；
  - `soulbase-qos`：把请求量/字节/扫描行回报到预算；
  - `soulbase-observe`：暴露读写延迟、事务失败率、索引命中率等指标。

------

#### **3. 数据模型与命名规范（Data Model & Naming）**

- **命名空间策略**：**单 NS / 单 DB** 承载多租户，**租户隔离靠列与索引**（`tenant` 字段 + 组合索引）；禁止“每租户一库”避免迁移/运维复杂度。
- **表 & 记录 ID**：SurrealDB 采用 `table:id` 形式；推荐：
  - 表名：`snake_case`；
  - 记录 ID：`<table>:<tenant>_<ulid>`（将 `TenantId` 前缀嵌入 ID，便于物理分布与前缀扫描）。
- **Schema 模式**：关键业务表 `SCHEMAFULL`，配合 `DEFINE FIELD` 校验；日志/事件表可 `SCHEMALESS` 但必须索引关键键。
- **图关系**：使用 `RELATE` / `->` / `<-` 表达边；边表统一前缀 `edge_`，并在两端记录 `tenant` 以强制同租户校验。
- **Envelope 持久化**：审计/证据事件表规范化字段：`tenant, trace_id, envelope_id, produced_at, payload, partition_key`。

------

#### **4. 不变式（Invariants）**

1. **参数化查询**：严禁字符串拼接 SurrealQL；统一**命名参数**绑定（防注入）。
2. **租户一致**：所有读写语句必须包含 `tenant = :tenant` 条件或在 ID 前缀中校验租户。
3. **显式事务**：跨表/多步写入统一通过 `BEGIN ... COMMIT`；失败必须 `ROLLBACK`（SurrealQL 原生支持）。([SurrealDB](https://surrealdb.com/docs/surrealql/transactions?utm_source=chatgpt.com))
4. **索引先行**：高频查询在上线前必须有 `DEFINE INDEX` 方案与回填策略；唯一性依赖 `UNIQUE` 索引。([SurrealDB](https://surrealdb.com/docs/surrealql/statements/define/indexes?utm_source=chatgpt.com))
5. **结构化迁移**：表/字段/索引定义以**迁移脚本**（SurrealQL）管理，禁止 “在线临时改表”。
6. **观测最小集**：**每次**往返记录 `latency_ms、rows、bytes、statement_kind、table、index_hit?、code`。
7. **错误稳定码**：连接/超时/网络 → `PROVIDER.UNAVAILABLE`；约束冲突 → `STORAGE.CONFLICT`；不存在 → `STORAGE.NOT_FOUND`；解析/参数错误 → `SCHEMA.VALIDATION_FAILED`；其它 → `UNKNOWN.INTERNAL`。
8. **可回放**：所有迁移/批量变更在审计表留痕（Envelope），可按 `produced_at` 回放。

------

#### **5. 能力与抽象接口（Abilities & Abstract Interfaces）**

> 具体 Traits 在 TD/RIS 实现，这里定义**行为口径**。

- **Datastore**：连接管理（WS/HTTP/TCP）、池化、NS/DB 选择、健康检查（`INFO FOR INDEX`/ping）；([SurrealDB](https://surrealdb.com/docs/surrealql/statements/info?utm_source=chatgpt.com))
- **Tx（事务）**：`begin()` → 多语句（含 `RELATE`） → `commit()/rollback()`；支持**幂等密钥**避免重放。
- **Repository**：
  - 基础 CRUD：`get(id)`、`create(table, doc)`、`update(id, patch)`、`delete(id)`；
  - 查询：参数化 `SELECT ... WHERE` 与分页；
  - **乐观并发**：版本字段（`ver`）或 `updated_at` 与 `WHERE ver = :ver`；
- **Graph**：`relate(from, edge, to, props)` / `unrelate(...)` / `out(from, edge)` / `in(to, edge)`；
- **Search**：
  - **全文**：全文索引定义与 `SEARCH` 查询；
  - **向量**：HNSW 向量索引定义与相似度检索接口（kNN / topK），面向嵌入向量。([SurrealDB](https://surrealdb.com/blog/v1-5-0-is-live?utm_source=chatgpt.com))
- **Migrations**：
  - 以 SurrealQL 脚本（`DEFINE TABLE/FIELD/INDEX/FUNCTION ...`）为单位；
  - 维护**迁移版本表**，保障幂等与回滚脚本；
- **Health**：
  - 查询慢日志/索引状态（`INFO FOR INDEX`）、连接重试/熔断、拓扑与容量水位。([SurrealDB](https://surrealdb.com/docs/surrealql/statements/info?utm_source=chatgpt.com))

------

#### **6. 事务与一致性（Transactions & Consistency）**

- 事务用于**跨记录/跨表**的一致变更；使用 SurrealQL `BEGIN TRANSACTION; ... COMMIT;`。([SurrealDB](https://surrealdb.com/docs/surrealql/transactions?utm_source=chatgpt.com))
- 写放大 & 冲突：采用**乐观并发**；冲突时返回 `STORAGE.CONFLICT`，上层可重试（指数退避）。
- **读隔离**：维持“读已提交”语义（结合 SurrealDB 引擎实现的乐观事务模型）。

------

#### **7. 索引/性能（Indexes & Performance）**

- `DEFINE INDEX`：单/复合/唯一索引；上线流程包含**回填/并发创建**与 `INFO FOR INDEX` 监控。([SurrealDB](https://surrealdb.com/docs/surrealql/statements/define/indexes?utm_source=chatgpt.com))
- **全文索引**：面向文本字段（检索接口封装 `SEARCH` 子句）。([SurrealDB](https://surrealdb.com/features?utm_source=chatgpt.com))
- **向量索引（HNSW）**：结合 SB-07 嵌入向量，定义维度/度量；提供 `knn(query_vec, k, filter)` 的**参数化**接口。([SurrealDB](https://surrealdb.com/blog/v1-5-0-is-live?utm_source=chatgpt.com))
- **数据建模建议**：
  - 高频查询：**租户 + 业务键** 的组合索引；
  - 事件/审计：`partition_key + produced_at DESC` 的二级索引，便于回放；
  - 图：边表在 `from/to` 上建立索引，限制跨租户。

------

#### **8. 多租户（Multi-Tenancy）**

- **强制** `tenant` 字段；所有查询**默认**带 `WHERE tenant = :tenant`；
- 记录 ID 前缀包含 `tenant`，配合索引加速 “租户内扫描”；
- 迁移脚本不得创建跨租户共享表结构的**可写视图**；跨租需求走 `soulbase-a2a`。

------

#### **9. 错误与映射（Errors & Mapping）**

| 分类                 | 典型场景                          | 稳定码                                                     |
| -------------------- | --------------------------------- | ---------------------------------------------------------- |
| 连接/网络/服务不可用 | 连接超时、TLS/WS 断开             | `PROVIDER.UNAVAILABLE`                                     |
| 约束冲突             | 唯一索引冲突、版本不匹配          | `STORAGE.CONFLICT`                                         |
| 资源不存在           | `SELECT ...` 空，`get(id)` 不存在 | `STORAGE.NOT_FOUND`                                        |
| 语法/参数错误        | SurrealQL 语法错误、参数缺失      | `SCHEMA.VALIDATION_FAILED`                                 |
| 事务失败             | 冲突/中断/回滚                    | `STORAGE.UNAVAILABLE` 或 `UNKNOWN.INTERNAL`（含 evidence） |

> 错误对外以公共视图返回；审计视图包含语句摘要（去敏）与绑定参数摘要。

------

#### **10. 指标与 SLO / 验收（Metrics, SLOs & Acceptance）**

- **指标**：
  - `storage_requests_total{kind=read|write|tx, table}`
  - `storage_latency_ms{p50,p95,p99, kind, table}`
  - `storage_rows/bytes{in|out}`、`storage_tx_rollback_total`
  - `storage_index_hit_ratio{index}`、`storage_vector_qps{k}`
  - `storage_errors_total{code}`
- **SLO**：
  - 读 p95 ≤ 15 ms；写 p95 ≤ 30 ms；事务提交 p95 ≤ 80 ms（不含网络长尾）；
  - `UNKNOWN.*` 占比 ≤ 0.1%；
  - 迁移失败可回滚率 100%；索引并发创建期间业务查询无大幅抖动（<10%）。
- **验收**：契约测试覆盖**参数化/事务/索引/向量检索/错误映射**，以及多租户过滤与审计回放。

------

#### **11. 安全与合规（Security & Compliance）**

- **参数化强制**：全部查询必须经命名参数绑定；
- **最小披露**：日志与审计记录仅存**语句哈希**与参数摘要；
- **数据留存策略**：与 `soulbase-qos`/配置模块联动，提供 TTL/归档脚本；
- **权限收口**：写路径一律经 `soulbase-interceptors` + `soulbase-auth` 审核；
- **备份/恢复**：定义快照导出/导入规约与停机窗口（不在本规约实现）。

------

#### **12. 关键交互序列（Key Interaction Sequences）**

**12.1 读（参数化查询）**

1. 上游携带 `tenant/trace` → Repository 绑定命名参数；
2. 执行 `SELECT ... WHERE tenant = $tenant AND ...`；
3. 记录指标与审计摘要 → 返回行集。

**12.2 写（事务 + 乐观并发）**

1. `BEGIN`；
2. `UPDATE ... SET ... WHERE id = $id AND ver = $ver`；
3. 受影响行 = 1 → `COMMIT`；否则 `ROLLBACK` → `STORAGE.CONFLICT`。([SurrealDB](https://surrealdb.com/docs/surrealql/transactions?utm_source=chatgpt.com))

**12.3 图关系**

1. `RELATE user:$uid->edge_likes->post:$pid SET tenant = $tenant, ...`；
2. 两端 `tenant` 校验一致；
3. 图遍历 `SELECT ->edge_likes->post WHERE tenant = $tenant ...`。

**12.4 向量检索**

1. 通过 SB-07 嵌入生成向量并写入向量字段；
2. `DEFINE INDEX ... HNSW ...`；
3. 查询时 `SELECT * FROM tbl WHERE tenant = $tenant ORDER BY similarity(vec, $qvec) LIMIT $k`（抽象封装）；([SurrealDB](https://surrealdb.com/blog/v1-5-0-is-live?utm_source=chatgpt.com))

**12.5 迁移**

1. 读取未执行的迁移脚本（SurrealQL）；
2. 逐条在**事务**中应用 `DEFINE TABLE/FIELD/INDEX`；
3. 写入迁移版本表（Envelope 审计）。

------

#### **13. 风险与控制（Risks & Controls）**

- **版本漂移**：SDK 与服务端不兼容 → 以兼容矩阵（2.0–2.3.8）守护并在 CI 做回归。([SurrealDB](https://surrealdb.com/docs/sdk/rust?utm_source=chatgpt.com))
- **索引创建长尾**：并发创建与 `INFO FOR INDEX` 监控 + 业务降级查询。([SurrealDB](https://surrealdb.com/docs/surrealql/statements/info?utm_source=chatgpt.com))
- **事务冲突**：采用重试策略（指数退避 + 上限）；
- **注入/拼接风险**：统一参数绑定；禁止模板拼接 SurrealQL。
- **多租户越权**：双重校验（where 子句 + ID 前缀），并在拦截器层进行二次拦截。

------

#### **14. 开放问题（Open Issues / TODO）**

- SurrealDB **LIVE** 查询/订阅是否在本层提供抽象（事件驱动读取）；
- HNSW 参数调优与在线重建索引的“平滑切换”策略；
- NS/DB 级资源配额与 QoS 的对齐（连接上限/速率/扫描行上限）；
- `SCHEMAFULL` 与 JSON-Schema 的**双重校验**边界与生成工具链；
- 与 `soulbase-benchmark` 的标准化回放集（包括向量/全文/图）。

------

> 上述规约以 **SurrealDB v2.3.x** 为基线（当前稳定 v2.3.8；Rust SDK 覆盖 2.0–2.3.8），并对其官方能力（SurrealQL、事务、索引/向量/全文）做了平台化抽象与治理约束，确保“**参数化、可观测、可回放**”与“**最小权限、默认拒绝**”的工程不变式。([SurrealDB](https://surrealdb.com/docs/surrealdb/installation?utm_source=chatgpt.com))

如果你确认没有遗漏，下一步我将输出 **SB-09-TD（技术设计）**，给出 Storage SPI、SurrealDB 适配层接口与命名参数规则、迁移脚本规范、索引/向量检索抽象、错误与指标出口的详细设计。
