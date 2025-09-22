# 《SB-03 · sb-config 开发总结》

## 1. 本轮完成情况
- 落地 sb-config crate：实现文件/环境变量/CLI 多源合并、默认值注入、Schema 校验、密钥解析接口（Noop）、只读快照、变更事件与原子切换。
- Loader 支持 load_with_prev，生成 ConfigUpdateEvent（含 version/checksum/changed_keys），并依据 Schema 注册表构建 reload 摘要。
- SchemaRegistry 提供字段元数据（默认值、敏感标识、热更等级），作为默认值注入与未知键拦截的 SSoT。
- ConfigSnapshot 支持类型化读取、Key 遍历与根值访问；SnapshotSwitch 保证快照原子替换与回滚。
- 单测覆盖“Schema 注册 + 默认值 + 双次加载零变更”场景，cargo test 全量通过（含 sb-types、sb-errors）。

## 2. 后续落地事项（下一阶段）
1. 远程 Source 适配器：补齐 Consul/etcd/S3/Git 等后端，实现版本/ETag、重试与认证（结合 sb-auth）。
2. Secrets Resolver 扩展：接入 Vault/KMS/ASM，完善密钥轮转、缓存与最小披露策略。
3. Watch/灰度方案：实现文件/远程 Watcher，针对 HotReloadRisky 键提供灰度发布、回滚与健康检查钩子。
4. 契约测试：在 sb-contract-testkit 中补充分层合并、Schema 校验、敏感字段屏蔽、热更限制等用例，并提供快照事件回放脚本。
5. 观测与门禁：与 sb-observe 集成加载/热更指标，结合 CI 的“未知键/未知错误码 grep”脚本形成上线前校验。

## 3. 参考文件
- 实现：crates/sb-config/src
- 测试：crates/sb-config/tests/basic.rs
- 事件：crates/sb-config/src/events.rs
