### **文档 SB-06：soulbase-sandbox（受控执行 / Controlled Execution & Evidence）**

#### **第一部分：功能逻辑规约**

------

#### **0. 目标与范围（Objectives & Scope）**

- **目标**：为 Soul 全生态提供一套**最小权限（least privilege）**、**可审计（evidence-first）**、**可回放（replayable）**的受控执行底座，统一承载**工具执行（Tools）**与**系统级行动（Computer-Use）**的能力封装、授权落地与证据闭环：
  1. **能力声明 → 授权核验 → 执行隔离 → 证据采集 → 预算扣减/回退** 的端到端链路；
  2. 与 `soulbase-auth` 的 **AuthZ/Quota/Consent** 一体化；
  3. 与 `soulbase-tools` 的 **Manifest/SideEffect/SafetyClass** 严格对齐与强约束；
  4. 与 `soulbase-interceptors`、`soulbase-observe` 对齐 **最小披露、稳定错误、可观测标签**。
- **范围**：定义**能力模型（Capabilities）**、**授权票据（Grants）**、**执行配置（Profiles）**、**证据模型（Evidence）\**与\**风控策略（Guards）**；覆盖**文件系统/网络/浏览器/进程/系统资源**等常见副作用域。
- **非目标**：本模块**不内置** OS 容器/虚拟化实现（Docker/VM）；采取 **SPI + 适配器** 方式接入具体隔离载体（如 WASI/wasmtime、本地受限 runner、Headless Browser 守护进程等）。

------

#### **1. 功能定位（Functional Positioning）**

- **Policy → Enforcement 的落点**：把来自 `soulbase-auth` 的**许可（Scopes/Consent/Quota）\**与 `soulbase-tools` 的\**能力声明**落到**可执行的隔离环境**与**可验证的行为边界**上。
- **证据单一真相源（SSoT）**：所有外部行动产出**标准化 Evidence**，通过 `soulbase-observe` 写入审计/指标，支持回放。
- **默认拒绝（deny-by-default）**：未显式授权的能力与资源**一律禁止**。

------

#### **2. 系统角色与地位（System Role & Position）**

- 所处层级：**基石层 / Soul-Base**；被 `soulbase-tools`、内核策略（工具调用）、以及需要副作用的业务模块直接消费。
- 关系：
  - **与 soulbase-tools**：工具 Manifest 的 `permissions/side_effect/safety_class/input_schema/output_schema` 必须映射到 Sandbox 的 **Capabilities/Guards**；
  - **与 soulbase-auth**：授权请求（资源=`soul:sandbox:*` + 具体能力），返回 **Grant/Quota**；
  - **与 soulbase-interceptors**：在执行前后挂接 **Envelope** 种子、标准头、错误规范化；
  - **与 soulbase-qos**：消耗预算（调用次数/网络字节/文件写入大小/CPU 秒等）；
  - **与 soulbase-observe**：证据/指标/追踪统一；
  - **与 soulbase-config**：执行策略（白/黑名单、限额、路径映射、域名出站清单）受配置/热更约束。

------

#### **3. 核心概念与数据模型（Core Concepts & Data Model）**

**3.1 Capabilities（能力域）**（最小集）

- `fs.read:{path}` / `fs.write:{path}` / `fs.list:{path}`（带路径映射/只读/配额）
- `net.http:{host[:port], scheme}`（明示出站域名与协议，仅允许 GET/HEAD/POST 的白名单变体）
- `browser.use:{scope}`（受控 Headless Browser 能力：打开页面、截图、提取；禁脚本注入/下载默认关闭）
- `proc.exec:{tool}`（受控子进程白名单；默认关闭 shell；仅参数白名单）
- `sys.gpu:{class}`（若涉及 GPU 推理/渲染，限定可用队列/配额）
- `tmp.use`（使用隔离临时目录）

> 每个能力都有**资源/动作/属性**三元：如 `resource="soul:fs:/workspace/tmp" action="read" attrs={max_bytes:1MB}`。

**3.2 SafetyClass（风险级别）**

- `Low`（只读、无副作用，如 fs.read/tmp.use）
- `Medium`（有限副作用，如 net.http GET 到白名单域，browser.use 只读操作）
- `High`（写入/修改系统状态，如 fs.write/proc.exec、下载到磁盘、跨域抓取）

**3.3 SideEffect（副作用类型）**

- `None | Read | Write | Network | Filesystem | Browser | Process`

**3.4 Grant（授权票据）**

- 来自 `soulbase-auth` 的**短期可撤销许可**：`granted_capabilities[]` + `tenant/subject` + `expires_at` + `budget{calls, bytes, tokens, cpu_ms}`。
- **DecisionKey** 绑定（避免越权复用），支持撤销（revocation）。

**3.5 Profile（执行配置）**

- 具体执行时的**合成视图**：`Grant ∩ Tool.Manifest ∩ PolicyConfig` → `Profile`（允许的能力+约束+限额+路径/域名映射）。

**3.6 Evidence（证据模型）**（最小必填字段）

- `envelope_id, tenant, subject_id, tool_name, call_id, profile_hash`
- `capability`（域+动作+资源）
- `inputs_digest`（输入摘要/尺寸/类型）、`outputs_digest`（输出摘要/尺寸/类型）
- `side_effects`（文件/网络/浏览器/进程的操作清单摘要）
- `budget_used`（bytes/calls/cpu_ms/gpu_ms）
- `policy_version_hash`（策略/白名单版本）
- `produced_at/finished_at`、`status`（ok/denied/error + code）

------

#### **4. 不变式（Invariants）**

1. **默认拒绝**：无 Grant/超出 Grant 范围/过期/撤销 → 直接拒绝。
2. **强绑定**：Grant 绑定 `tenant/subject/tool_name/call_id`，不可跨调用复用。
3. **最小权限**：Profile 取 **三者交集**（Grant ∩ Manifest ∩ PolicyConfig），并且取最窄约束。
4. **证据先行**：执行前后均产出 Evidence（开始记录+结束记录），失败也必须有证据。
5. **可回收**：所有临时产物默认写入隔离空间（tmp），在会话/调用结束后清理。
6. **无直通 IO**：禁止未声明的 stdout/stderr 外泄；输出必须走**受控通道**（回传 payload/产物摘要）。
7. **出站白名单**：网络/浏览器出站必须命中白名单；DNS rebinding/重定向跨域视为拒绝。
8. **参数与内容守护**：执行前对 URL/路径/命令做 Schema & 策略校验；大型输出/下载默认阈值限制。

------

#### **5. 能力与抽象接口（Abilities & Abstract Interfaces）**

> 本节为**行为口径**，具体 Traits / SPI 在 TD & RIS 落地。

- **Profile 生成**：
   输入：`Subject/Consent + Grant + Tool.Manifest + PolicyConfig`；输出：`Profile`（能力矩阵/白名单/限额/映射/超时）。
- **Pre-Exec 审核**：
   对即将执行的动作（如 HTTP 请求、文件写入、子进程）进行 **Schema + Policy** 校验，必要时**二次同意**校验。
- **执行器（Executors）**：
  - `FsExecutor`：只读/写入、目录映射、文件大小/数量/路径模式限制；
  - `NetExecutor`：仅允许白名单域名/端口/方法、请求头过滤、响应大小限制、MIME 白名单；
  - `BrowserExecutor`：无头浏览器能力（打开/截图/提取 DOM / 跳转），禁 JS 注入/下载；
  - `ProcExecutor`：白名单可执行名 + 参数模板校验 + 资源限额（CPU/内存/时长），默认关闭；
  - `TmpExecutor`：隔离临时目录管理。
- **预算计量**：
   统一计量 `bytes/calls/cpu_ms/gpu_ms/file_count`，调用中实时扣减，超限即中止。
- **证据采集**：
   自动记录**操作摘要与指纹**，生成 `Envelope<EvidenceEvent>`；失败时记录 `error_code` 与 `cause` 摘要。
- **终止/回收**：
   超时/取消/异常 → 发送终止信号，强制退出；清理隔离空间；写入结束 Evidence。

------

#### **6. 指标与 SLO / 验收（Metrics, SLOs & Acceptance）**

- **执行安全**：
  - 未授权直通（bypass）率 **= 0**；
  - 出站命中白名单率 **= 100%**；
  - 高风险能力未带 Consent 的执行 **= 0**。
- **证据完整性**：
  - 每次执行产生 **开始/结束** 两条 Evidence；缺失率 **= 0**。
- **性能开销**：
  - Profile 合成 p95 **≤ 2ms**；Pre-Exec 校验 p95 **≤ 3ms**（不含浏览器启动）；
  - 证据写出在**异步**路径（不阻塞主执行）。
- **稳定错误覆盖**：
  - 拒绝与失败**100%** 归入 `soulbase-errors` 稳定码（`POLICY.* / SANDBOX.* / PROVIDER.*` 等）。
- **验收**：
  - 黑盒：未授权拒绝、白名单拦截、配额超限、Consent 缺失、证据生成、回放一致性；
  - 压测：在目标 QPS 下附加开销不超过预算；
  - 回放：基于 Evidence 可复现关键副作用摘要。

------

#### **7. 依赖与边界（Dependencies & Boundaries）**

- **上游**：`soulbase-auth`（Grant/Quota/Consent）、`soulbase-tools`（Manifest）、`soulbase-config`（策略/白名单/限额）、`soulbase-interceptors`（上下文/Envelope）。
- **下游**：`soulbase-observe`（证据/指标），可能的隔离载体（WASI/wasmtime、headless browser 守护、受限 runner）。
- **边界**：不提供容器编排；不直接替代工具实现；不做业务策略（只执行与约束）。

------

#### **8. 风险与控制（Risks & Controls）**

- **路径穿越/越权** → 路径正则/根目录绑定/拒绝符号链接/写入仅限 tmp & 映射；
- **DNS/重定向越界** → 解析前检查域名，重定向链全程校验白名单；
- **命令注入** → 仅允许白名单 `proc.exec`，参数模板化（禁止自由 shell），禁环境继承；
- **大文件/炸弹** → 请求/响应/文件大小/数量阈值 & 解压禁用/限制；
- **长尾执行** → 全链路超时与取消；
- **证据遗漏** → Begin/End 双事件与异常兜底；
- **隐私外泄** → Header/Query/Body 过滤、下载默认关闭、输出脱敏义务（与 `obligations` 协同）。

------

#### **9. 关键交互序列（Key Interaction Sequences）**

**9.1 工具调用（受控 HTTP 抓取示例）**

1. `soulbase-tools` 读取工具 Manifest（`net.http` + `Read`，Safety=Medium，SideEffect=Network）；
2. `soulbase-auth` 决策返回 Grant（白名单域名、最大 bytes、TTL、预算）；
3. Sandbox 合成 Profile（∩ Config 白名单/限额）→ 记录 **BeginEvidence**；
4. Pre-Exec 校验 URL 与方法/域名/大小阈值 → 通过；
5. `NetExecutor` 执行请求，流式计量 bytes，用尽预算即中止；
6. 产出 **EndEvidence**（inputs/outputs 摘要、budget_used、policy_hash）；
7. 结果通过 `soulbase-interceptors` 规范化返回，对外错误走稳定码；指标/证据入库。

**9.2 受控文件写入**

1. Manifest 声明 `fs.write`（Safety=High）；
2. 若需要 Consent（高风险），在 AuthZ 阶段校验并绑定到 Grant；
3. 路径必须在允许映射下，写入大小/数量受限；
4. EndEvidence 记录写入文件的相对路径/大小摘要与 hash（避免敏感内容外泄）。

**9.3 受控浏览器使用**

1. `browser.use`（只读：打开/截图/提取文本）；
2. 禁用脚本注入与下载，限制导航深度、并发标签页、截图分辨率与频率；
3. Evidence 记录页面 URL/导航链摘要/截图尺寸哈希。

------

#### **10. 开放问题（Open Issues / TODO）**

- **隔离载体选型**：WASI/wasmtime 与本地轻量 runner 的统一抽象与回退策略；
- **浏览器沙箱**：跨站限制、Cookie/Storage 洁净化、对抗 fingerprint 的最小实现；
- **差分隐私/脱敏**：在 Evidence 与回传结果中的标准化脱敏策略；
- **GPU 配额**：`sys.gpu` 的单位与计量（GPU 秒/显存 MB*s）对齐 `soulbase-qos`；
- **离线回放**：是否提供“模拟执行器”（只读模式，基于 Evidence 重建摘要行为用于审计/测试）。

------

> 本规约与先前文档**同频共振**：以 **Policy → Enforcement → Evidence** 为主轴，确保“**最小权限、默认拒绝、证据闭环、稳定错误**”。若你确认无误，下一步我将输出 **SB-06-TD：技术设计**（Traits/SPI、Profile 合成流程、各执行器与 Guards、Evidence 结构、与 Auth/Tools/Observe 的接口），随后给出 **SB-06-RIS** 最小骨架。
