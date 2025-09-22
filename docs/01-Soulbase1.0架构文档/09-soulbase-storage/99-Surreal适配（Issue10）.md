# Surreal 适配（Issue 10 对齐）

> 在不改变原文结构的前提下，补充 SurrealDB 适配的仓储/事务/迁移规范，统一键/索引/租户强约束与参数化查询口径，并与 SB‑10/11/14/15 对齐。

## 仓储（Repository）规范

- 参数化查询：统一使用命名参数（如 `$tenant`, `$id`），严禁拼接 SQL/SurrealQL；
- 强租户约束：所有表/查询默认含 `tenant` 维度，必须在 WHERE 子句中强约束（或物化前缀分库策略）；
- 主键与唯一性：
  - 业务主键：`(tenant, resource_key)`；
  - 幂等去重：相关表建立 `UNIQUE (tenant, envelope_id)` 索引；
  - 账页唯一：`UNIQUE (tenant, envelope_id, line_kind)`（避免重复结算）；
- 软/硬删除：优先软删除（`deleted_at`），后台 TTL 清理；
- 图与向量：
  - 图：使用 `RELATE` 建边，边表统一 `Edge_{lhs}_{rhs}` 命名；
  - 向量：封装向量插入/检索接口，以便后续替换；
- 乐观并发：统一 `version`（或 `updated_at` + 比较）做 CAS；

## 事务（Tx）

- 事务边界：单租户事务；跨租户禁止；
- 顺序：业务写入 → Outbox 记录 → COMMIT；
- 回调：COMMIT 成功后触发缓存失效（见 Issue 07 附录）；
- 错误：统一映射到 SB‑02 稳定码（`TX.TIMEOUT/…`）。

## 迁移（Migrator）

- 版本化：
  - 表 `migrations{version, checksum, applied_at, author, notes}`；
  - 每次迁移包含 `up/down` 与 `checks`（索引存在、字段非空等）；
- 回滚策略：失败时可 `down` 至上一版本；
- 数据安全：涉及重算/改模的迁移需分批窗口 + 影子表/回填；
- 审计：迁移事件写入 `Envelope<ConfigUpdateEvent>`（版本/校验和/耗时）。

## 模式与索引（示例）

- Outbox：`outbox{tenant, envelope_id, topic, payload, attempts, status, created_at}` + `UNIQUE(tenant, envelope_id)`；
- Idempotency：`idempo{tenant, envelope_id, hash, status, result_digest, ttl}` + `UNIQUE(tenant, envelope_id)`；
- Ledger：`ledger{tenant, envelope_id, line_kind, amount, currency, meta, created_at}` + `UNIQUE(tenant, envelope_id, line_kind)`；
- A2A 反重放：`a2a_replay{tenant, envelope_id, seq, nonce, ts}` + `UNIQUE(tenant, envelope_id, seq)`；

## 验收

- 集成测试：读写/事务/迁移通过；
- 索引与唯一约束生效；
- 强租户 WHERE 检查覆盖 ≥ 99.9% 查询（CI 合同校验）；
- 与 SB‑10/14/15 的去重/账页/反重放一致；
- 回滚脚本有效。

## 测试与运行

- 提供 `crates/sb-storage/tests/surreal_e2e.rs` 端到端用例，覆盖 Repository CRUD、关系图、向量检索、全文检索全链路；
- 运行前需设置以下环境变量指向可用的 SurrealDB 实例：
  - `SURREAL_URL`（如 `ws://127.0.0.1:8000` 或 `http://127.0.0.1:8000`）
  - `SURREAL_NAMESPACE` / `SURREAL_DATABASE`
  - `SURREAL_USERNAME` / `SURREAL_PASSWORD`（如使用匿名访问可忽略）
- 启动测试：`cargo test -p sb-storage --features surreal surreal_end_to_end_smoke`；未设置环境变量时测试自动跳过。
