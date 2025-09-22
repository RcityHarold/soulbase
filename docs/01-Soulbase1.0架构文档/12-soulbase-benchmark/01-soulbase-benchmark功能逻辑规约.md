### **文档 SB-12：soulbase-benchmark（基准与回放 / Benchmark · Replay · Regression Gate）**

#### **第一部分：功能逻辑规约**

------

#### **0. 目标与范围（Objectives & Scope）**

- **目标**：为 Soul 生态提供**统一的基准评测与回放框架**，在**可控、可复现实验环境**下，对 **LLM（SB-07）/Tools（SB-08）/Sandbox（SB-06）/Storage（SB-09）/Tx（SB-10）/Interceptors（SB-05）** 的**准确性、稳定性、时延与成本**进行量化评估，并作为**回归门禁（Regression Gate）\**与\**性能 SLO 守护**的权威依据。
- **范围**：
  1. **Benchmark Suite/Case** 定义与**数据集管理**（离线/合成/脱敏采样）；
  2. **Replay 引擎**（Golden Trace / Evidence 回放 / 影子流量 Shadow-Mode）；
  3. **度量与比较器**（Metric/Probe/Tolerance）与**报告器**（Report/Trend）；
  4. **回归门禁策略**与**基线管理**（Baseline/Pin 版本/环境指纹）；
  5. 与 `soulbase-observe`/`-contract-testkit` 的协同（证据、契约一致性）。

> 非目标：不替代线上 A/B 测试（可作为前置准入）；不做业务报警（交由 observe/告警系统）。

------

#### **1. 功能定位（Functional Positioning）**

- **统一评测语言**：用 Suite/Case/Probe/Tolerance 描述**同一套指标语义**，跨模块通用；
- **证据驱动回放**：基于 `Envelope<Evidence*>` 的**Golden Trace**回放，保证**可再现**；
- **门禁中枢**：将评测结果与**版本/配置/模型/价格表**绑定，变更前先过 Gate。

------

#### **2. 系统角色与地位（System Role & Position）**

- 层级：**基石层 / Soul-Base**；所有核心模块上线前的**统一入口**。
- 关系：
  - `soulbase-observe`：拉取或订阅 Evidence，生成 Golden Trace；产出基准报告指标。
  - `soulbase-contract-testkit`：并行执行**契约测试**（结构/错误语义），Benchmark 负责**性能/成本/质量**。
  - `soulbase-config/qos`：记录配置戳记与成本预算；场景/价目表作为输入。
  - `soulbase-llm/tools/sandbox/storage/tx/interceptors`：作为被评测对象（SUT）。

------

#### **3. 核心概念与数据模型（Core Concepts & Data Model）**

- **BenchmarkSuite**：一组**主题一致**的用例，具**环境约束**（模型/价目/开关）、**数据集**与**Probe 组合**。
- **Case**：单个评测用例，字段：`id, tenant, scenario (llm|tool|sandbox|storage|tx|http), input, expected?`。
- **Dataset**：Case 的集合与生成器（合成/脱敏/采样），带版本与摘要（sha256）。
- **GoldenTrace**：从 `Evidence`/Interceptors 采集的**规范化请求-响应-证据**序列，字段：`inputs_digest, outputs_digest, policy_hash, usage/cost, latency`。
- **Baseline**：上一次通过 Gate 的**基线结果**（指标向量 + 统计分布 + 版本/配置/模型指纹）。
- **Probe**：度量器，如 `latency_ms`, `cost_usd`, `accuracy`, `json_valid`, `schema_valid`, `tool_success_rate`, `sandbox_budget_bytes`, `tx_retry_count`。
- **Comparator**：比较器，定义**公差**（Tolerance）：绝对/相对/百分位阈值（`<= Xms / <= Y%`）。
- **Report**：评测报告，含**全量指标**、**与 Baseline 的差异**、**Gate 结论**与**可视化摘要**。

------

#### **4. 不变式（Invariants）**

1. **Schema-first**：Case/GTrace/Report 全部结构化，具 `schema_ver`。
2. **可复现**：固定 `seed/model_alias/config_snapshot`；LLM 回复采用**结构化比较** + 规范化（空白/顺序/数值误差容忍）。
3. **最小披露**：GoldenTrace 仅存摘要（hash/长度/指标），敏感字段脱敏；原文不入库。
4. **环境指纹**：任何评测结果必须携带 `config_version/checksum`、`provider/model_id`、`sandbox/profile_hash`、`policy_hash` 等标签。
5. **单一真相源**：所有指标以 `soulbase-observe` 的**同一标签语义**产出。
6. **门禁可解释**：Gate 结论给出**明确的“失败条目 + 指标偏差 + 公差定义”**，而非黑盒。

------

#### **5. 能力与接口（Abilities & Abstract Interfaces）**

- **Suite/Case 管理**：定义/导入/导出；支持 YAML/JSON 配置与 Rust 生成器；版本化与摘要。
- **数据集生成器**：合成（Prompt template/工具参数组合）、脱敏（Redactor）与采样（Stratified）。
- **Replay 引擎**：
  - **Golden 回放**：按 GTrace 重现输入/上下文/策略，校验输出摘要与预算/时延；
  - **影子流量**：实时复制入站请求到“影子 SUT”（不影响主路）；
  - **试验沙箱**：将工具/浏览器/网络写入**重定向到临时根**，避免污染。
- **Probe/Comparator**：
  - Metric 采集：调用 observe SDK；
  - 质量指标：LLM 输出 JSON/Schema 校验、关键字/规则、评分器（可插拔）。
- **报告与 Gate**：
  - 生成 `Report{ suite, stats{probe->dist}, delta, gate{pass|fail, reason} }`；
  - 写回 `Baseline`（仅在 `pass` 且被明确“Pin”为基线时）。
- **CI 集成**：命令行/库 API；失败时输出**最小差异**与重现命令。

------

#### **6. 指标与 SLO / Gate（Metrics & Gate）**

- **指标族**（最小集）：
  - `bench_latency_ms{scenario,case}`（端到端）；
  - `bench_cost_usd{scenario,case}`（LLM/工具/网络聚合）；
  - `bench_success{scenario,case}`（工具/Tx 成功标识）；
  - `bench_schema_ok{scenario,case}`（结构化输出有效率）；
  - `bench_retry_count{scenario,case}`（Tx/Outbox 重试次数）；
  - `bench_budget_bytes{dir}{scenario,case}`（Sandbox）。
- **Gate 策略**（示例）：
  - p95 时延**上涨 ≤ 10%**；
  - 成本**上涨 ≤ 5%**；
  - 结构化输出有效率 **≥ 99.5%**；
  - 工具成功率**不下降**（或 ≥ 指定阈值）；
  - Tx 重试/死信**不增加**；
  - 任何**稳定错误码**分布出现**新峰值**需阻断。

------

#### **7. 依赖与边界（Dependencies & Boundaries）**

- **上游**：`soulbase-observe`（Evidence/指标输入）、`soulbase-config`（环境）、`soulbase-qos`（成本权重）
- **下游**：报告存储（交给 `soulbase-storage` 或构建制品）；CI 系统
- **边界**：不直接修改被测系统；回放写路径必须沙箱化或模拟。

------

#### **8. 风险与控制（Risks & Controls）**

- **不可复现**：固定种子/模型别名/配置戳记；对 LLM 文本用**规范化比较**（trim/空白/数值误差/键序）。
- **数据泄露**：GoldenTrace 严格脱敏；仅保留摘要与指标。
- **回放污染**：文件/网络/浏览器写入**全部重定向**到临时根或禁写。
- **标签漂移**：所有指标通过 observe SDK，标签在白名单内校验。
- **过拟合基线**：Baseline 以**分布 + 置信区间**表达，不对单次漂移过度敏感。

------

#### **9. 关键交互序列（Key Interaction Sequences）**

**9.1 Suite 执行（离线回放）**

1. 载入 `Suite/Dataset/Baseline`；
2. 为每个 `Case` 构建上下文（tenant/trace/config snapshot/model alias）；
3. 通过模块 SDK 调用（LLM/Tools/Sandbox/Storage/Tx/HTTP）→ **采集 Probe**；
4. 规范化输出与校验 → 聚合指标 → 与 Baseline 比较；
5. 输出 `Report` 与 Gate 结论。

**9.2 影子流量（Shadow）**

1. Interceptors 复制入站请求到影子目标（不执行副作用或重定向到沙箱）；
2. 影子结果仅用于统计，不影响主路；
3. 生成影子报告与差异热图。

**9.3 GoldenTrace 采集与刷新**

1. 从 observe 的 Evidence 流中抽样/筛选；
2. 经过脱敏/规范化，生成新的 `GoldenTrace` 数据集；
3. 与 Baseline 成对存档（用于长期趋势）。

------

#### **10. 开放问题（Open Issues / TODO）**

- **自动探针**：依据 Suite 自动选择最佳 Probe/阈值（基于历史分布/异常检测）。
- **多模质量指标**：图像/音频工具的客观指标（SSIM/PSNR/WER）。
- **分布式回放**：大规模回放的调度/并发/速率限制策略。
- **成本归因**：跨模块细粒度成本的统一账本与权重模型。

------

> 如认可此规约，我将按“三件套”继续输出 **SB-12-TD（技术设计）**，给出 Suite/Case/Dataset/GTrace/Probe/Comparator/Report 的结构、回放引擎与沙箱化策略、门禁算法与 CI 集成接口，并随后提供 **SB-12-RIS（最小可运行骨架）**。
