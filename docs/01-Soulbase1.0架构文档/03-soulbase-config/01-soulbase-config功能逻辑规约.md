### **文档 SB-03：soulbase-config（配置与密钥管理 / Configuration & Secrets）**

#### **第一部分：功能逻辑规约**

------

#### **0. 目标与范围（Objectives & Scope）**

- **目标**：为 Soul 生态（SoulseedAGI、Soul-Auth、Soul-Hub、SoulBrowser、AI 记忆等）提供**统一、可验证、可回滚**的**应用配置与密钥管理**能力：多源加载→分层合并→Schema 校验→安全分发→受控热更新→全链路审计。
- **范围**：
  - 配置项与密钥的**定义、加载、合并、校验、读取、热更新、版本化、审计与回滚策略**。
  - 支持多源：**文件**（YAML/TOML/JSON）、**环境变量**、**进程参数**、**远程配置**（Consul/etcd/S3/Git）、**密钥管理**（Vault / KMS / Secrets Manager）。
  - 输出**类型化读取接口**与**只读快照（Config Snapshot）**的消费约束。
- **非目标**：不绑定具体后端实现（这些在技术设计与 RIS 中以适配器体现）；不实现业务策略/动态开关逻辑（仅提供**Feature Flag**载体与读取口径）。

------

#### **1. 功能定位（Functional Positioning）**

- **SSoT（单一真相源）**：为每个进程实例提供**单一、可审计**的配置真相源，并能生成**稳定快照**。
- **Schema-first**：所有配置均应有**显式 Schema**与**默认值策略**；未注册的键一律拒绝。
- **Secure-by-Design**：密钥**隔离、加密、零日志**；敏感字段最小披露。
- **可演进**：对“可热更新”与“仅启动可读”配置**显式分级**；提供**灰度/回滚**路径。

------

#### **2. 系统角色与地位（System Role & Position）**

- 所处层级：**基石层 / Soul-Base**，被所有服务与内核直接依赖。
- 关系：
  - 依赖 `sb-types` 提供**Envelope/Trace/Subject** 用于配置事件审计。
  - 与 `soulbase-errors` 对齐错误域（加载/合并/解密/校验失败的标准错误码）。
  - 向 `soulbase-interceptors` 暴露**配置快照标识**用于请求关联与可观测。
  - 与 `soulbase-observe` 对接**变更事件与指标**。
  - 与 `soulbase-auth` 协作**远程配置/密钥拉取**的认证与授权。

------

#### **3. 核心概念与数据模型（Core Concepts & Data Model）**

- **Config Source（配置源）**：`File` / `Env` / `CliArgs` / `RemoteKV` / `SecretStore`。
- **Layer（分层）**：固定顺序合并：`Defaults < File < RemoteKV < Env < CliArgs`（后者覆盖前者）。
- **Schema**：每个命名空间（如 `server.*`、`auth.*`、`llm.*`）拥有**结构与类型约束**（JSON Schema / IDL）。
- **Secret**：受保护的键（`*.secret`, `*.token`, `*.password`, `*.key` 等），**仅以密文或外部引用**（如 `secret://vault/path#key`）出现。
- **Snapshot（配置快照）**：在运行时**不可变的读取视图**，包含：`version`、`checksum`、`provenance（来源）`、`issued_at`、`reload_policy`。
- **Reloadable Class（热更新级别）**：
  - `BootOnly`（仅启动加载，如端口、数据库方言、模型目录结构）
  - `HotReloadSafe`（可无损热更，如阈值、配额参数、路由表权重）
  - `HotReloadRisky`（需**闸门/灰度**策略，如模型切换、Sandbox 策略等）
- **Feature Flag**：布尔/变体开关与**受众（tenant/region/percent）**；本模块仅提供结构与读取，策略在上层。

------

#### **4. 不变式（Invariants）**

1. **显式 Schema**：没有 Schema 的键**不得暴露**给消费者。
2. **分层可重复**：每次构建快照必须遵循固定**合并序**，结果可复现。
3. **敏感隔离**：敏感键**不落磁盘明文**、**不写日志**、**不通过公共 JSON 导出**。
4. **读写分离**：对外仅提供**只读快照**；写入/变更仅通过**受控 Loader/Watcher**完成。
5. **可追溯**：每个快照具备**版本号与校验和**，并通过 `Envelope<ConfigUpdateEvent>` 形成**可回放**的审计记录。
6. **热更显式**：只有声明为 `HotReload*` 的键才允许在运行时变更；否则**拒绝热更**。
7. **失败不破坏**：热更失败**回退到上一个有效快照**；启动加载失败**拒绝启动**（fail-fast）。
8. **最小必要披露**：对调用方暴露**必要键集**，支持**命名空间级**访问控制。

------

#### **5. 能力与抽象接口（Abilities & Abstract Interfaces）**

> 仅定义能力与行为；具体 Traits/适配器详见 TD/RIS。

- **注册与默认值**：
  - 注册**命名空间 Schema**与默认值；支持**必填/可选**、**枚举与范围**。
- **源适配与加载**：
  - File（`*.yml|*.toml|*.json`）、Env 前缀、CliArgs、RemoteKV（Consul/etcd/S3/Git 等）、SecretStore（Vault/KMS/ASM）。
- **合并与校验**：
  - 按 Layer 顺序合并；对结果做 Schema 校验；对敏感键进行**解密/拉取**。
- **快照构建与发布**：
  - 生成 `Snapshot{version, checksum, provenance, issued_at, reload_policy}`；对外提供**类型化读取**与**命名空间读取**。
- **热更新**：
  - 对 `HotReloadSafe/Risky` 键支持**监听→校验→原子切换**；`Risky` 需灰度（可按租户/百分比）与回滚指令。
- **审计与观测**：
  - 每次快照发布/回滚生成 `Envelope<ConfigUpdateEvent>`；指标包含加载时间、失败率、热更成功率等。
- **访问控制**：
  - 提供**命名空间级**访问裁剪（例如：“工具运行时”仅可读 `tool.*` 子树）。
- **故障策略**：
  - 启动期：加载失败→`SCHEMA.VALIDATION_FAILED` / `PROVIDER.UNAVAILABLE` / `AUTH.FORBIDDEN` 等标准错误码→**拒绝启动**；
  - 运行期：热更失败→保留旧快照，并上报错误与事件。

------

#### **6. 指标与 SLO / 验收（Metrics, SLOs & Acceptance）**

- **SLO-1（启动）**：95% 实例在 **< 2s** 内完成配置加载与校验；100% 保证**一致快照**。
- **SLO-2（热更）**：`HotReloadSafe` 热更成功率 **≥ 99.9%**；失败自动回退，上报事件 ≤ 60s。
- **SLO-3（安全）**：0 次敏感键泄露到日志/公共导出；密钥**轮转延迟 ≤ 5m**（可配置）。
- **SLO-4（一致性）**：同一版本号在相同环境合并结果**字节级一致**（checksum 相等）。
- **验收**：契约测试覆盖**分层合并/Schema 校验/敏感剔除/热更策略/回退**；回放审计日志可复现快照历史。

------

#### **7. 依赖与边界（Dependencies & Boundaries）**

- **上游**：外部配置与密钥系统（Vault/KMS/SecretsManager/Consul/etcd/S3/Git）；认证由 `soulbase-auth` 提供令牌/会话。
- **下游**：所有服务/内核模块；`soulbase-interceptors` 用快照标识增强请求上下文；`soulbase-observe` 消费变更事件与指标。
- **边界**：
  - 不负责业务开关策略（仅提供 Flag 值）；
  - 不负责网络证书签发（只管理证书位与轮转策略）；
  - 不内置存储（远程源通过适配器接入）。

------

#### **8. 风险与控制（Risks & Controls）**

- **配置漂移**（多实例不一致）→ **控制**：统一合并顺序 + 校验 + 版本校验和 + 启动期强一致；
- **部分热更导致行为不一致** → **控制**：原子切换/双缓冲 + `Risky` 灰度 + 观测告警；
- **密钥泄露** → **控制**：敏感键黑名单、零日志、只在内存持有、KMS 封装、最小披露；
- **远程源不可用** → **控制**：启动期 fail-fast；运行期使用**最后已知良好快照（Last Known Good）**；
- **Schema 漂移** → **控制**：SemVer 管理 Schema；契约测试守护；未注册键拒绝。

------

#### **9. 关键交互序列（Key Interaction Sequences）**

**9.1 启动加载（Boot Strap）**

1. 注册 Schema 与默认值；
2. 依顺序加载：`Defaults → File → RemoteKV → Env → CliArgs`；
3. 替换敏感引用（从 Vault/KMS 拉取或解密）；
4. Schema 校验通过→产出 **快照 vX.Y.Z**（含 checksum/provenance）→发布；
5. 生成 `Envelope<ConfigUpdateEvent>` → 送 `soulbase-observe`。

**9.2 热更新（Hot Reload）**

1. Watcher 发现变更→拉取→合并→仅对 `HotReload*` 键校验；
2. 若通过：原子切换快照→发布 `ConfigUpdateEvent`；
3. 若失败：拒绝切换并回退→上报错误和事件。

**9.3 密钥轮转（Secret Rotation）**

1. SecretStore 发布新版本→短周期轮询或回调→更新内存密钥→触发依赖组件“软刷新”；
2. 任何密钥在日志/公共导出中均以**占位符**出现（如 `***`）。

------

#### **10. 开放问题（Open Issues / TODO）**

- 跨语言 SDK（TS/Go/Java）的**Schema 校验与类型绑定**统一规范；
- 与 GitOps（Config as Code）的对接：**变更审查/签名**与**双人复核**；
- 多环境（dev/stage/prod）**覆盖策略**与**跨环境差异报告**的标准格式；
- Feature Flag 的**百分比灰度**与**租户定向**是否在本模块提供最小支持（仍不含策略）。

------

> 以上为 `soulbase-config` 的**功能逻辑规约**。若你确认无遗漏，我将按“三件套”流程，继续输出 **SB-03-TD：技术设计文档**（crate 结构、Source/Watcher/Resolver/Validator 抽象、快照模型、变更事件、热更原子切换与回退机制、与 secrets 的接口）——不包含代码骨架，按你的指示在下一步再给 RIS。
