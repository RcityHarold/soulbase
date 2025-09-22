### **文档 SB-13：soulbase-contract-testkit（契约测试套件 / Contract Tests & Compatibility Gate）**

#### **第一部分：功能逻辑规约**

------

#### **0. 目标与范围（Objectives & Scope）**

- **目标**：提供一套**跨模块、跨版本、可自动化**的契约测试基座，用来验证 **接口形状（Schema）**、**错误语义（稳定错误码）**、**行为不变式（Invariants）** 与 **兼容性矩阵（Compatibility Matrix）**，从而在**模块升级/供应商替换/配置热更**时，提前阻断破坏性变更与语义漂移。
- **范围**：
  1. **ContractSpec**（契约规范）与**Case/Clause**（用例/条款）模型；
  2. **Runner** 与 **Adapter**（对接被测 SUT：LLM/Tools/Sandbox/Storage/Tx/Auth/Interceptors）；
  3. **断言器（Asserter）**：Schema/错误码/不变式/幂等/重试/安全屏蔽；
  4. **兼容性矩阵**：版本×配置×供应商的通过谱；
  5. **报告与 Gate**：CI 门禁、变更说明与破坏性变更提示（Diff）。
- **非目标**：不替代性能/成本评测（由 **SB-12 Benchmark** 承担）；不负责 SUT 的部署与弹性（依赖各模块自有基建）。

------

#### **1. 功能定位（Functional Positioning）**

- **契约真相源**：把“**Schema-first + 稳定错误码 + 不变式**”转为**可执行契约**；
- **进化护栏**：在**升级/热更**前，自动比对**当前实现**与**ContractSpec**，发现破坏性变更即**阻断合并**；
- **一致性粘合剂**：对齐 `soulbase-*` 全家桶的**统一标签与错误语义**，避免“各测各的”。

------

#### **2. 系统角色与地位（System Role & Position）**

- 层级：**基石层 / Soul-Base**（测试与合规维度）；
- 关系：
  - **SB-02 errors**：检查**稳定错误码**与 **HTTP/gRPC 映射**一致性；
  - **SB-01 types / SB-08 tools / SB-07 llm / SB-06 sandbox / SB-09 storage / SB-10 tx / SB-04 auth / SB-05 interceptors**：提供各域 **Adapter** 与预置 **ContractSpec**；
  - **SB-12 benchmark**：并行执行**性能/成本**评测；契约测试负责**功能/语义**；
  - **SB-11 observe**：采集契约执行的观测指标与证据摘要，形成**可审计**的测试轨迹。

------

#### **3. 核心概念与数据模型（Core Concepts & Data Model）**

- **ContractSpec**：某接口/模块的**契约规范**，包含：
  - **Schema 契约**：请求/响应/事件的 JSON-Schema/IDL；
  - **错误契约**：输入边界与异常路径对应的**稳定错误码**与建议状态码；
  - **不变式**：幂等/最小披露/默认拒绝/租户一致/参数化查询等规则；
  - **示例工件**：样例请求/响应与红线屏蔽清单。
- **Clause（条款）**：独立可执行的断言集合（例如“输入缺字段 → SCHEMA.VALIDATION_FAILED”）。
- **Case（用例）**：一组合规输入 + 期望断言，按场景聚合（正向/负向/边界/恶意）。
- **Matrix（兼容性矩阵）**：在版本×配置×供应商维度的通过/未通过分布（用于发布决策）。
- **Report**：一次测试运行的结果与 Diff（对比上一个参考版本或上一次通过的基线）。

------

#### **4. 不变式（Invariants）**

1. **Schema-first**：契约中的全部 I/O 必须有 Schema 且**可校验**；
2. **稳定错误语义**：每条负向 Case 必须有**稳定错误码**断言；`UNKNOWN.*` 比例必须在阈值内（默认 0）；
3. **最小披露**：对外响应/日志/证据只含**公共视图**与摘要；合规字段（密钥/PII）**不得**出现；
4. **幂等与重试**：声明幂等的操作在重复提交时**结果等价**；可重试错误在指数退避后**具确定性**；
5. **多租户一致**：所有写路径断言**tenant 一致性**；跨租户拒绝；
6. **可重放**：所有契约执行产生 `Envelope<ContractEvent>`，可用于离线复盘与审计。

------

#### **5. 能力与接口（Abilities & Abstract Interfaces）**

- **Spec 装载**：从代码/文件（YAML/JSON）或内建模板加载 `ContractSpec`；
- **Adapter SPI**：为每个被测域提供统一 **Adapter**（发起调用，返回受控公共视图）；
- **Asserter**：
  - **SchemaAsserter**：请求/响应/事件 Schema 校验；
  - **ErrorAsserter**：错误码 & HTTP/gRPC 映射；
  - **InvariantAsserter**：不变式（幂等/拒绝默认/最小披露/参数化）；
  - **SecurityAsserter**：红线屏蔽（敏感字段）与租户一致性；
- **Runner**：顺序/并发执行 Case；注入**环境指纹**与随机种子；
- **Diff & Gate**：与历史基线对比，生成“破坏性变更”报告并反馈到 CI。

------

#### **6. 指标与 SLO / 验收（Metrics, SLOs & Acceptance）**

- **指标**：
  - `contract_cases_total{module,pass}`，`contract_errors_total{code}`，`contract_unknown_total`，`contract_schema_violations`；
- **SLO**：
  - `UNKNOWN.*` 为 0；
  - Schema 断言通过率 100%；
  - 不变式断言通过率 100%；
  - 兼容矩阵覆盖≥ 95%（按声明维度）；
- **验收**：
  - PR/Release 前**必须通过**契约测试；
  - 破坏性变更需附**ADR & 迁移计划**，并在 Matrix 中标注“从版本 X 起不兼容”。

------

#### **7. 依赖与边界（Dependencies & Boundaries）**

- **上游**：`soulbase-config`（环境快照）、`soulbase-observe`（证据/指标）、`soulbase-errors`（码表）
- **下游**：被测模块/服务（SUT）
- **边界**：不负责部署；不运行破坏性的真实副作用（对 Tools/Sandbox/Storage/Tx 走**沙箱或影子通道**）。

------

#### **8. 风险与控制（Risks & Controls）**

- **契约与实现偏离**：以 Spec 为准；Runner 对**所有 I/O**强制 Schema 校验；
- **错误码稀释**：负向 Case 库覆盖全部常见错误场景（缺字段/类型错/越权/配额/路径越界/超时…），避免 `UNKNOWN.*`；
- **隐私泄露**：对响应/日志施加**红线屏蔽**；报告只含摘要；
- **兼容死角**：引入 **Matrix 驱动**，对关键 Provider/版本/配置交叉验证；
- **成本失控**：与 Benchmark 集成，控制测试规模与速率，必要时以 **Shadow** 降低对生产的影响。

------

#### **9. 关键交互序列（Key Interaction Sequences）**

**9.1 LLM 契约（示例）**

1. 加载 `llm.chat` 的 `ContractSpec`（消息 Schema、工具提案只读、结构化输出格式）；
2. 运行 Case：
   - 正向：`response_format=json` → **SchemaValid=TRUE**；
   - 负向：上下文溢出 → **LLM.CONTEXT_OVERFLOW**；
3. Asserter 校验 **Schema/错误码/最小披露**；
4. 产出 `Evidence<ContractEvent>` 与指标。

**9.2 Tools 契约（示例）**

1. Manifest 中声明 `input_schema/output_schema/permissions/capabilities`；
2. 运行 Case：
   - 缺参数 → `SCHEMA.VALIDATION_FAILED`；
   - 未授权 → `AUTH.FORBIDDEN`；
   - 沙箱拦截 → `SANDBOX.CAPABILITY_BLOCKED`；
3. 校验幂等（带相同 Idempotency-Key 二次调用 → 结果等价）。

**9.3 Storage 契约（示例）**

1. 只允许**参数化查询**；
2. 运行 Case：字符串拼接/跨租户 → 断言 **拒绝或报错**；
3. 索引与唯一约束的冲突 → `STORAGE.CONFLICT`。

------

#### **10. 开放问题（Open Issues / TODO）**

- **跨语言生成器**：从 ContractSpec 自动生成 TS/Go/Java 客户端与 Mock 服务；
- **更强的“差异断言”**：对 JSON 结构提供“**语义差异**”而非文本差异；
- **供应商变更风险模型**：当 Provider API 变化时，自动生成提示与**变更向导**；
- **长期演进地图**：ContractSpec 与 Benchmark 的合并报告（功能+性能）的标准格式。

------

> 若本规约方向一致，下一步我将输出 **SB-13-TD（技术设计）** 的落地接口（ContractSpec/Clause/Case/Runner/Asserter/Adapters）、兼容性矩阵模型与报告生成，以及随后提供 **SB-13-RIS（最小可运行骨架）**。
