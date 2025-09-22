# SB-09 `soulbase-storage` 开发总结（2025 Q1）

## 一、状态结论
- ✅ 功能清单已收口：完成 SurrealDatastore/Session/Tx、Repository、Graph、Vector、Search、Migrator 适配及错误映射、指标接线、Mock 支撑；
- ✅ 静态检查通过：`cargo fmt`、`cargo check`；
- ✅ 集成测试：新增 `surreal_end_to_end_smoke`（外部 SurrealDB 场景，可用环境变量跳过或执行）；
- ⚠️ 后续关注：生产部署尚需补充连接重试策略、索引迁移灰度发布方案。

## 二、功能与接口要点
1. **Surreal 连接与会话**
   - 新增 `SurrealDatastore` 统一池化、租户强约束、指标打点；
   - `Session::query_json` / `Tx::execute` 返回结构化 JSON + `QueryResult`，便于上层仓储消费；
   - 错误映射全部落地 SB-02 稳定码，日志/指标聚合一致。
2. **Repository / Graph / Vector / Search**
   - `build_filter_clause` 支持 `$and` / `$or` / `$not` / `$in` / `$contains` 等条件树，默认注入 `tenant`；
   - 仓储分页返回 `Page { items, next }` 游标，满足增量拉取；
   - Graph/Vector/Search 统一指标标签（`table` / `kind` / `tenant`），mock 与真实实现行为一致。
3. **迁移 & 管理**
   - `SurrealMigrator` 定义迁移版本表、事务化 `apply_up`/`apply_down`；
   - In-memory Migrator 供单元测试验证迁移顺序与版本追踪；
   - `filtered_bindings` 避免内部参数污染 SurrealQL 绑定。

## 三、测试与验证
- **端到端测试**：`cargo test -p sb-storage --features surreal surreal_end_to_end_smoke`
  - 需设置环境变量：`SURREAL_URL`、`SURREAL_NAMESPACE`、`SURREAL_DATABASE`，如需鉴权再提供 `SURREAL_USERNAME`、`SURREAL_PASSWORD`；
  - 用例自动建表/索引、执行 CRUD / Graph / Vector / Search，并在结束后清理数据。
- **Mock 单测**：`cargo test -p sb-storage --tests` 覆盖仓储 CRUD、全文检索、迁移追踪等；
- **静态检查**：`cargo fmt` + `cargo check` 常规通过。

## 四、使用指引（最小示例）
```rust
let config = SurrealConfig::default()
    .with_pool(16)
    .with_credentials(SurrealCredentials::new("root", "root"));
let datastore = SurrealDatastore::connect(config).await?;
let repo = SurrealRepository::<Doc>::new(datastore);
let tenant = TenantId::from("tenantA");
let doc = repo.create(&tenant, &new_doc).await?;
```
- Graph/Vector/Search 可分别实例化 `SurrealGraph::new`、`SurrealVectorIndex::new`、`SurrealSearch::new`；
- 若需自定义指标，可实现 `StorageMetrics` 并通过 `with_metrics` 注入。

## 五、待跟进事项（Backlog）
1. **生产级配置**：支持多集群、连接重试、超时/指数退避策略；
2. **数据迁移工具链**：补齐命令行工具读取 `migrations/` 并输出审计；
3. **索引健康监控**：定期执行 `INFO FOR INDEX` 采样 + 指标输出；
4. **Vector/Fulltext 扩展**：暴露 HNSW 参数、查询阈值等可配置项；
5. **CI 集成**：在 CI 中启动轻量 Surreal 服务，自动执行 `surreal_end_to_end_smoke`。

## 六、版本与发布
- 推荐 tag：`sb-storage v0.1.0-alpha`（启用 `surreal` feature 视为实验性能力）；
- 发布前需同步更新工作区 README，并在环境准备文档中说明测试所需环境变量；
- 已完成与 `sb-tools` 的 Schema/错误码联动修复。

> 负责人：Storage Team（@dev-storage-squad），2025-02。
