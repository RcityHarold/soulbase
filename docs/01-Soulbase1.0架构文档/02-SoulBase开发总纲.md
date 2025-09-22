# 《Soulbase · 开发总纲（平台版 · 融合定稿）》

> 目标：把**平台能力**（鉴权/配额、存储/索引/回放、Outbox/Saga/幂等等、LLM 执行与治理、工具执行与沙箱、指标与日志、加密与证据、出网与实时）**一次性上收**到 Soulbase；用**薄腰接口**（Thin-Waist）对外暴露**稳定契约**，让上层 **SoulseedAGI** 只做“认知/编排/策略”。
>  风格：**契约先行、Append-Only、可回放、可解释、可灰度**。本总纲是平台侧**唯一工程宪法**。

------

## 0. 使命 / 边界 / 不做什么

- **使命**：让一切“带副作用”的能力在平台侧**可执行、可治理、可审计、可复用**。
- **边界（核心职责）**
  1. **统一入口与拦截器**：AuthN/Z、RateLimit、Compliance、Quota、Audit（SB-04/05/14）。
  2. **账本与索引**：Append-Only 事件仓、索引/回放基线（SB-09）。
  3. **事务与幂等等**：Outbox/Saga、预扣/对账、去重与重放（SB-10）。
  4. **LLM 执行/治理**：`stream|complete|embed|rerank`、双信息流解析、结构化守护、用量/成本、统一错误与指标（SB-07）。
  5. **工具执行/沙箱**：注册/发现、三段式幂等、并发/流式/后台护栏、缓存、成对入账（SB-08/06/19）。
  6. **观测与日志**：统一指标族/错误域、追踪/日志/证据（SB-11/02/17/18）。
  7. **出网与实时**：受控 egress、LIVE 订阅、网络策略（SB-19）。
- **不做**：业务路由/编排、上下文构型、分叉决策、Prompt 逻辑、策略与叙事（全部在 **AGI**）。

------

## 1. 不可变工程约束（共同宪法，平台履约）

1. **Append-Only**：任何纠错/撤回/补偿均以**新事件**表达；旧账不改写。
2. **幂等等**：外部幂等键 `(tenant, idem_key/envelope_id)`；内部幂等键 `(ac_id, ic_seq, step_id)`。
3. **Ledger+Outbox 同事务**：事件追加与 Outbox.enqueue 要么一起提交，要么一起失败。
4. **SyncPoint 一等公民**：平台侧 inbox 能表达**顺序/去重/空洞容忍**与“**一次吸收**”。
5. **决策稳定/可回放**：所有决策类接口回显 `*_digest`（canonical JSON → sha256）与 `config_snapshot_hash/version`。
6. **Final 唯一 & 迟到回执**：Final 后回执**只审计**，不改 AC。
7. **禁止线扫**：Graph/Storage 查询必须声明索引；无索引计划直接拒绝。
8. **六锚统一**：`tenant_id / envelope_id / config_snapshot_hash&version / (session_id,sequence_number) / access_class+provenance / schema_v (+ supersedes*)`。

------

## 2. 模块矩阵与职责（SB-xx）

| 模块                           | 职责                                                         | 对外薄腰                                     |
| ------------------------------ | ------------------------------------------------------------ | -------------------------------------------- |
| **SB-04/05 Gateway**           | 统一入口、拦截器链（AuthN/Z、RateLimit、Compliance、Quota、Audit） | 透明，不直接暴露                             |
| **SB-09 Storage/Graph**        | 事件仓（Append-Only）、索引/回放、Graph/Recall 抽象（Vec+Sparse+Causal） | `repo.append(event)` / `graph.recall(query)` |
| **SB-10 Tx/Outbox**            | Outbox/Saga、预扣/对账、去重/重放                            | 内部调用                                     |
| **SB-07 LLM**                  | 执行与治理：`stream                                          | complete                                     |
| **SB-08 Tools**                | 工具注册/发现、三段式幂等、并发/流式/后台护栏、缓存、事件成对入账 | `/tools.list                                 |
| **SB-11 Observe/SB-02 Errors** | 指标/日志/追踪、统一错误域与映射                             | `/observe.emit`（内部）                      |
| **SB-17/18 Blob/Crypto**       | 证据/大对象与摘要校验（EvidencePointer/AEAD/JWS）            | `/blob.get                                   |
| **SB-19 Net/LIVE**             | 受控 egress、LIVE 订阅、网络策略                             | `/live.subscribe`（内部）                    |

------

## 3. 薄腰接口（Thin-Waist）——**唯一跨层入口**

> 只列最小字段；完整 schema 放在 `docs-/04-contracts/`。

### 3.1 Storage/Graph

- `POST /repo/append` → `{ ok }`（**Append-Only**；拒绝 UPDATE/DELETE）
- `POST /graph/recall` → `{ items[], indices_used[], query_hash }`（**必须**回显索引；禁线扫）

### 3.2 LLM（SB-07）

- `POST /llm/stream` / `POST /llm/complete`
   **Request**：六锚、`idempotency_key`、`model_id`、`messages`、`params`、`response_format`、`tool_specs?`、`allow_sensitive?`、预算
   **Response**：`provider_meta / usage / cost / degradation_reason?`（流式用增量 `ChatDelta`）
- `POST /llm/embed` / `POST /llm/rerank`（维度/分数/用量/成本）

### 3.3 Tools（SB-08）

- `GET /tools.list?scene=...` → `[ToolDef...] + policy_digest`（**可见=可用**）
- `POST /tools.precharge`（预扣；**同幂等键只扣一次**）
- `POST /tools.execute` / `POST /tools.reconcile`
- `POST /tools.emit_events`（`ToolCalled/Responded/Failed` **成对事件**）

### 3.4 Observe / Blob / Crypto

- `POST /observe.emit(name, labels, val)`
- `GET/PUT /blob`（签名 URL）；`POST /crypto.verify`（checksum/签名校验）

**统一错误域（示例）**：`AUTH.FORBIDDEN | TENANT.MISMATCH | QOS.RATE_LIMITED/EXCEEDED | IDEMPOTENT.BUSY | STORAGE.CONFLICT | LLM.TIMEOUT | TOOL.INTERNAL | SCHEMA.VALIDATION_FAILED | SAFETY.BLOCK | CONTEXT.SCAN_FORBIDDEN …`

------

## 4. 目录与工作区（Workspace）

```
soulbase/
├─ docs-/
│  ├─ 00-Authority/         # 事件词表、错误域、指标族（唯一权威）
│  ├─ 04-contracts/         # 薄腰接口 schemas（JSON/YAML）与 examples
│  ├─ 05-runbooks/          # 运维手册、回放流程、事故手册
│  └─ 99-changelog/
├─ crates/
│  ├─ sb-gateway/           # 拦截器 & API Gateway
│  ├─ sb-storage/           # 事件仓/索引/回放/graph
│  ├─ sb-tx/                # Outbox/Saga/幂等
│  ├─ sb-llm/               # LLM 执行与治理
│  ├─ sb-tools/             # 工具执行与治理
│  ├─ sb-observe/           # 指标 & 日志
│  ├─ sb-blob-crypto/       # 证据与加密
│  └─ sb-live-net/          # 出网/LIVE
└─ tests/
   ├─ contracts-validate/   # AJV/开放 API 验证 & 负契约
   └─ contract-runner/      # 端到端契约 Runner（按 use-case）
```

------

## 5. TDD 六步工作流（平台版）

1. **Study**：从 `docs-/04-contracts` 拉 schema → 读取 `00-Authority` 错误域/指标名。
2. **Test**：先写 **contract tests**（AJV + runner），覆盖成功/失败/边界/幂等等/降级外泄。
3. **Implement**：落 **SPI/Facade/Adapter**，保证 **Append-Only/幂等等/索引必经**。
4. **Refactor**：抽离 provider/存储适配，确保错误映射与指标上报一致。
5. **Integrate & Verify**：以 AGI 的薄腰 Mock 跑一轮回放；比对 `*_digest/Explain`。
6. **Confirm**：合入主干；更新 `00-Authority` 与 `99-changelog`。

**CI 双工作流**

- `contracts-validate`：schema → AJV → 负契约（拒绝线扫/未声明索引）
- `contract-runner`：以 fixtures 跑 end-to-end；对照 `digest/usage/cost/indices_used/degradation_reason`

------

## 6. 红线（Hard-lines）

- **禁止线扫**：Graph/Storage 请求必须带 `indices_used`；未声明或未命中直接 4xx。
- **Final 后迟到回执不改状态**：只写 `LateReceiptObserved`。
- **结构化守护默认开启**（LLM）：“严格拒绝/自动修复”可灰度，失败回 `SCHEMA.VALIDATION_FAILED`。
- **工具只提案不执行（LLM → Tool）**：`tool_specs` 只声明 schema；执行需要工具总线二次授权。
- **证据只指针不原文**：EvidencePointer 强校验（checksum/access_policy）；大对象走 Blob。
- **多租户首列**：所有主/边表索引首列恒为 `tenant_id`。

------

## 7. 里程碑与交付（建议 4～6 周）

### M1（平台骨架，1.5 周）

- Gateway+拦截器（AuthN/Z/RateLimit/Quota）
- Storage：Append-Only/索引与回放基线；Graph `recall`（索引必经）
- Observe：指标出口（`llm.*`, `tool.*`, `repo.*`, `graph.*`）

### M2（LLM/Tools 最小可用，2 周）

- LLM：`stream|complete|embed` + 双流解析 + 结构化守护 + 用量/成本/错误域/指标
- Tools：`list|precharge|execute|reconcile|emit_events` + 三段式幂等 + 成对入账
- Outbox/Saga：预扣/对账/去重/重放；同事务保障

### M3（证据/加密/回放/灰度，1.5 周）

- Blob/Crypto：EvidencePointer 校验、AEAD/JWS
- Live/Net：受控 egress & 订阅上限/心跳/清退
- Replayer：以 fixtures 回放 AGI 测试集；`*_digest` 一致
- 配置中心/灰度与回滚：`config_snapshot` + 双栈版本

**每个模块 DoD（样例：SB-07 LLM）**

- **功能**：`stream|complete|embed` 终态一致；结构化守护开关 + 回退策略；
- **治理**：预扣/对账幂等等；重试/熔断边界；安全审核与最小披露；
- **观测**：`llm_first_token_ms/llm_stream_complete_ms/llm_retry_count/llm_usage_tokens{input,output}`；
- **契约**：AJV 过 & runner 用例全绿；错误映射与降级外泄齐全。

------

## 8. 指标与错误域（统一命名）

- **指标族**
  - `gateway.*`（auth/rate/latency）
  - `repo.*`（append/scan_forbidden/index_hit）
  - `graph.*`（recall_latency / indices_used / query_hash）
  - `tx.*`（outbox_enqueue / saga_compensate / idempotent_hit）
  - `llm.*`（first_token / complete / usage / cost / degrade）
  - `tool.*`（precharge / execute / reconcile / cache_hit）
  - `observe.*`（emit / series / error）
- **错误域**（SB-02）：见 `docs-/00-Authority/errors.md`（作为唯一权威）。

------

## 9. 回放与灰度

- **回放**：以 `fixtures/` 的事件与上下文，按 `*_digest`/`manifest_digest`/`indices_used`/`query_hash` 对齐；覆盖 LL M/Tools/Storage 关键路径。
- **灰度/回滚**：`router_config_digest/config_snapshot` 双栈运行；回滚必须得出**相同 digest**（等价重放）。

------

## 10. 落地清单（按模块拆分 PR）

- **SB-04**：拦截器链（AuthN/Z/RateLimit/Quota）与可配置化；
- **SB-09**：Append-Only + `indices_used` 强制；`SCAN_FORBIDDEN` 负契约；
- **SB-10**：Outbox/Saga 与预扣/对账；
- **SB-07**：LLM 三口 + 双流解析 + 结构化守护 + 用量/成本 + 指标；
- **SB-08**：Tools 薄腰 + 三段式幂等 + 成对入账 + 缓存 vary-by；
- **SB-11**：指标出口 + 错误映射；
- **SB-17/18**：EvidencePointer 校验；
- **SB-19**：LIVE 订阅与 egress 代理；
- **Runner & Replayer**：端到端契约 Runner；回放一致率脚本。

------

### 结语

自此，**平台与编排分责清晰**：**Soulbase** 兜住全部工程复杂度与合规风险，以**薄腰契约**服务上层；**AGI** 专注“会用能力的智慧”。本总纲落地后，团队可以并行推进模块 PR，CI 以契约驱动协作，确保每条路径**可复现、可解释、可回放**。
 （下一部分将输出 **《SoulseedAGI · 开发总纲（ · 编排版）》**，与本总纲镜像配套。）
