# 06-soulbase-sandbox · 开发总结

## 1. 当前实现概述
- 沙箱 crate 已实现从 **能力声明 → Profile 合成 → 策略守卫 → 执行器编排 → 预算治理 → 证据落地** 的闭环，符合 SB-06 功能规约与 Issue04/05 的“Config 快照贯穿”与“默认拒绝+能力白名单”要求。
- 与其他基座模块的协同：
  - **sb-auth**：消费 Grant/Consent/Quota。ProfileBuilder 将授权票据与 Manifest/PolicyConfig 取交集，并在执行前校验 Grant 过期/撤销与高风险 Consent。
  - **sb-config**：PolicyConfig 内含 policy_hash/config_version/config_hash，执行时锁定快照并写入 Evidence。
  - **sb-errors**：拒绝/异常统一映射到 `SANDBOX.* / POLICY.* / AUTH.*` 稳定错误码，并对外仅输出公共视图。
  - **sb-tools**：能力、SafetyClass 与 SideEffect 集合与工具 Manifest 对齐，Guard 保证“声明即约束”。
  - **sb-observe**：通过 EvidenceSink SPI 输送 Begin/End 事件，labels_from_error 提供统一观测标签。
- Orchestrator（Sandbox）串联 ProfileBuilder、PolicyGuard、BudgetMeter、RevocationWatcher 与执行器，默认挂载 Noop 适配器，同时支持注入真实实现。

## 2. 已完成功能
- **模型与契约**：Capability/CpKind/SideEffect/Grant/Budget/Profile/ToolManifest 全量实现，Profile.hash 提供策略快照摘要；新增 `DataDigest`、`SideEffectRecord` 便于证据摘要化。
- **ProfileBuilder**：实现 Grant∩Manifest∩Policy 的能力交集、限额最窄化、Risk 等级最大化、白名单/路径映射合并，并自动写入 policy_hash 与 config 快照。
- **策略守卫**：
  - Guard 校验路径归一化、域名后缀/方法白名单、工具白名单；对 tmp/root 映射执行限定。
  - Manager 在执行前检查 Grant 过期/撤销、Consent、Budget 预留，并将 `requires_consent` 与 SafetyClass 结合。
- **执行器 SPI**：定义 CancelToken/ExecUsage/ExecResult；内置 Fs/Net/Browser/Proc/Tmp 执行器实现：
  - FsExecutor：字节/文件数限额、Base64 摘要、SideEffect 记录。
  - NetExecutor：HTTPS/HTTP 校验、私网/环回阻断、方法白名单、请求体限额与 SideEffect 摘要。
  - BrowserExecutor：导航/截图默认只读，结合限额生成证据。
  - ProcessExecutor：工具白名单、参数安全校验、超时、输出截断摘要。
  - TmpExecutor：按限制创建隔离 tmp，并产生 SideEffect 摘要。
- **证据闭环**：EvidenceBuilder 产出 Begin/End 双事件，包含 profile_hash、policy/config 快照、输入/输出 Digest、SideEffect 列表、预算使用与持续时间；EvidenceSink SPI 支持可插拔落地。
- **预算治理**：BudgetMeter SPI (reserve/commit/rollback)，Manager 在失败路径回滚并映射 Denied/Error；NoopMeter/RecordingMeter 覆盖最小与测试场景。
- **Revocation/Consent**：提供 RevocationWatcher SPI，默认 Noop；Manager 在执行前检查 Grant 是否被吊销、Consent 是否存在/未过期。
- **配置结构**：PolicyConfig 支持 capabilities/safety/side_effects/limits/whitelists/mappings/timeout/默认值及策略快照三元组。
- **测试**：
  - Profile 合成与限额收敛。
  - Guard 拒绝越权域名。
  - 集成测试验证预算预留/提交、证据字段、SideEffect 摘要与成功路径。
  - `cargo test -p sb-sandbox` 全量通过。

## 3. 功能现状
沙箱已满足 SB-06 的“可执行 + 证据闭环”要求，并遵守默认拒绝、最小权限、预算/证据贯穿、稳定错误映射等原则。现阶段实现为生产可用骨架，可直接支撑工具执行与 Computer-Use 的安全落地，并向后兼容 futuras 扩展（真实隔离载体、QoS 集成等）。

## 4. 后续可选增强
1. **真实隔离器接入**：为 Net/Proc/Browser 执行器提供 reqwest/wasi/chromium 等 adapter，并通过 feature 控制灰度。
2. **证据落库与回放**：结合 sb-storage/sb-observe 将 EvidenceEvent 标准化入账，并提供回放工具链（契约测试/审计复现）。
3. **高级风控**：增强路径/符号链接校验、HTTP 重定向链白名单、命令模板/资源限制、压缩炸弹/大型响应流控等守卫策略。
4. **QoS 深度集成**：启用 qos feature 与 sb-qos 联动，对 CPU/GPU/字节预算统一持久化并联动账页。
5. **结构化守护**：结合 sb-tools Schema 约束做执行前/执行后 schema 校验，提高输出可解释性。

> 结论：当前版本已达到 SB-06 规约的“最小权限 + 默认拒绝 + 证据闭环”标准，可作为 Soul 平台受控执行底座落地使用；后续可按需接入实际隔离运行时与 QoS/审计系统以进一步增强能力。
