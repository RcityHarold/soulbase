# Cache 失效接线（Issue 07 对齐）

> 在不改变原文结构的前提下，明确存储侧的缓存失效事件与粒度。

## 变更事件

- 统一发布 `StorageChangeEvent{ tenant, namespace, resource, keys[], op }`：`op ∈ {insert, update, delete, migrate}`；
- 对批量操作按资源维度合并事件，避免风暴；
- 迁移（migrate）事件应选择合理窗口，避免大规模缓存抖动。

## 失效策略

- 使用 `tenant/namespace/resource` 前缀或反向索引定位相关缓存键；
- 最小必要失效：仅针对受影响 keys/prefix；
- 失败重试 + 幂等保护；
- 与 Tx 回调协同（提交成功后再失效）。

## 验收

- 写后强失效生效及时，命中率/陈旧度指标可观测；
- 大批量变更操作不引发缓存风暴；
- 与 SB‑16 的 Key 规范兼容。
