# 文档 SB-13-TD：`soulbase-contract-testkit` 技术设计

（ContractSpec / Clause / Case / Runner / Asserter / Adapters · 兼容性矩阵 · 报告生成）

> 对应规约：SB-13
>  目标：给出**可落地**的接口与数据结构，覆盖 ContractSpec 建模、Case/Clause 执行、断言器体系、各域适配器、Runner 编排、兼容性矩阵与报告生成；与 `soulbase-*` 模块的**稳定错误码、统一标签、最小披露**不变式同频。

------

## 1. Crate 结构与 Feature 规划

```
soulbase-contract-testkit/
  src/
    lib.rs
    errors.rs
    model/                # ContractSpec / Clause / Case / Matrix / Report
      spec.rs
      case.rs
      clause.rs
      matrix.rs
      report.rs
    asserter/             # 断言器
      mod.rs
      schema.rs
      error.rs
      invariant.rs
      security.rs
      idempotency.rs
      retry.rs
    adapters/             # 被测系统适配（SUT）
      mod.rs
      llm.rs              # SB-07
      tools.rs            # SB-08 (+ SB-06)
      storage.rs          # SB-09
      tx.rs               # SB-10
      auth.rs             # SB-04
      http.rs             # SB-05
    runner/
      mod.rs
      plan.rs             # 执行计划与并发/隔离
      engine.rs           # 执行器
    io/
      loader.rs           # Spec/Case YAML/JSON 载入
      report_json.rs      # 报告生成（机器可读）
      report_console.rs   # 控制台摘要（CI 友好）
    prelude.rs
```

**Features**

- `yaml`（以 serde_yaml 载入 spec/case）
- `schema_json`（依 schemars 校验 JSON Schema）
- `observe`（与 SB-11 对接，输出运行证据与指标）

------

## 2. 核心数据模型（`model/`）

### 2.1 ContractSpec（契约规范，`spec.rs`）

```rust
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ContractSpec {
  pub id: String,                     // "llm.chat.v1"
  pub version: String,                // SemVer of the spec itself
  pub domain: Domain,                 // Llm | Tools | Sandbox | Storage | Tx | Auth | Http
  pub schema: IoSchema,               // 请求/响应/事件的 JSON-Schema/IDL
  pub errors: Vec<ErrorClause>,       // 稳定错误码条款
  pub invariants: Vec<Invariant>,     // 行为不变式（幂等/参数化/默认拒绝等）
  pub examples: Vec<ExampleArtifact>, // 样例请求与公共视图响应
  pub matrix_axes: MatrixAxes,        // 兼容性维度定义
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Domain { Llm, Tools, Sandbox, Storage, Tx, Auth, Http }

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct IoSchema {
  pub request: serde_json::Value,     // JSON Schema（或 IDL 占位） 
  pub response: serde_json::Value,    // 公共视图的 Schema
  pub events: Vec<(String, serde_json::Value)>, // "AuthDecision" -> Schema
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ErrorClause {
  pub when: serde_json::Value,        // 触发条件描述（Case 输入 filter）
  pub code: String,                   // 稳定错误码（e.g. "SCHEMA.VALIDATION_FAILED"）
  pub http_status: Option<u16>,       // 建议映射
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Invariant {
  DenyByDefault,                      // 未授权/不在白名单 → 拒绝
  Idempotent,                         // 幂等键重复请求结果等价
  TenantConsistent,                   // 跨租户拒绝
  ParametrizedQuery,                  // 禁止字符串拼接（Storage）
  MinimalDisclosure,                  // 只返回公共视图
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ExampleArtifact {
  pub title: String,
  pub request: serde_json::Value,
  pub response_public: serde_json::Value,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MatrixAxes {
  pub versions: Vec<String>,          // SUT 版本
  pub providers: Vec<String>,         // LLM/存储/网关供应商
  pub configs: Vec<String>,           // 配置快照/开关组合
}
```

**YAML 片段（示例）**

```yaml
id: "tools.browser.fetch.v1"
domain: Tools
version: "1.2.0"
schema:
  request: { "$schema":"http://json-schema.org/draft-07/schema#", "type":"object", "properties": { "url": { "type":"string", "format":"uri" } }, "required":["url"] }
  response: { "type":"object", "properties":{"status":{"type":"integer"}, "body":{"type":"string"}}, "required":["status"] }
errors:
  - { when: {"missing":"url"}, code: "SCHEMA.VALIDATION_FAILED", http_status: 422 }
  - { when: {"unauthorized":true}, code:"AUTH.FORBIDDEN", http_status:403 }
invariants: [ MinimalDisclosure, DenyByDefault ]
matrix_axes: { versions: ["1.0.0","1.1.0"], providers:["chromium"], configs:["default","safe-mode"] }
```

### 2.2 Case & Clause（`case.rs`, `clause.rs`）

```rust
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Case {
  pub id: String,
  pub spec_id: String,                 // 关联 ContractSpec
  pub tenant: String,
  pub input: serde_json::Value,        // 满足 request Schema
  pub expect: Vec<Clause>,             // 条款断言组合
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Clause {
  SchemaOk,                            // 响应匹配 response Schema
  ErrorIs { code: String },            // 稳定错误码
  FieldEq { path: String, value: serde_json::Value }, // JSON 路径 = 值
  InvariantHold { inv: Invariant },    // 不变式成立
}
```

### 2.3 兼容性矩阵 & 报告（`matrix.rs`, `report.rs`）

```rust
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MatrixKey { pub version: String, pub provider: String, pub config: String }

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MatrixResult {
  pub key: MatrixKey,
  pub pass: bool,
  pub totals: u32,
  pub passed: u32,
  pub failed: u32,
  pub unknown: u32,                    // UNKNOWN.* 次数
  pub by_code: std::collections::BTreeMap<String, u32>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RunReport {
  pub spec_id: String,
  pub matrix: Vec<MatrixResult>,
  pub cases: Vec<CaseReport>,
  pub summary: Summary,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CaseReport {
  pub case_id: String,
  pub ok: bool,
  pub code: Option<String>,            // 若失败/错误，记录稳定码
  pub violations: Vec<String>,         // 违反的条款
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Summary { pub totals: u32, pub passed: u32, pub failed: u32, pub unknown: u32 }
```

------

## 3. 断言器（`asserter/*`）

### 3.1 SPI（`asserter/mod.rs`）

```rust
#[async_trait::async_trait]
pub trait Asserter: Send + Sync {
  async fn assert(&self, ctx: &RunCtx, case: &Case, resp: &PublicResp) -> Vec<String>; // 返回违例描述
}

pub struct RunCtx {
  pub spec: ContractSpec,
  pub matrix_key: MatrixKey, // version/provider/config
}

pub struct PublicResp {
  pub code: Option<String>,            // 稳定错误码（若有）
  pub http_status: Option<u16>,
  pub body: serde_json::Value,         // 公共视图
  pub events: Vec<(String, serde_json::Value)>, // 发生的事件及公共视图
}
```

### 3.2 Schema 断言（`schema.rs`）

- 校验 `case.input` 满足 `spec.schema.request`；
- 成功时校验 `resp.body` 满足 `spec.schema.response`；
- 事件（如 `AuthDecision`）匹配事件 Schema；
- 违例输出 `SCHEMA: request|response|event <reason>`。

### 3.3 错误码断言（`error.rs`）

- 若命中 `ErrorClause.when`（以简单匹配器或在 Case 上显式标注），则断言 `resp.code == clause.code`；
- HTTP 映射（如有）一致；违例输出 `ERROR: expected <code> got <resp.code>`。

### 3.4 不变式断言（`invariant.rs`）

- **DenyByDefault**：标记为“无授权/白名单失配”的 Case，应返回 `AUTH.FORBIDDEN|POLICY.DENY_*`;
- **ParametrizedQuery**：Storage 适配器返回 `parametrized=true` 标志；否则违例；
- **TenantConsistent**：Case 指定跨租户访问时应拒绝；
- **MinimalDisclosure**：检查响应体不含敏感键（由 SecurityAsserter 的屏蔽表驱动）。

### 3.5 安全屏蔽断言（`security.rs`）

- 对 `resp.body` 与事件公共视图递归扫描敏感键（`authorization|token|password|secret|cookie`）；出现即违例；
- 支持 `allowlist` 例外（配置自 Spec/Runner）。

### 3.6 幂等 & 重试断言（`idempotency.rs`, `retry.rs`）

- **Idempotent**：对幂等 Case，同一 `Idempotency-Key` 调用两次，公共视图**等价**；
- **Retry**：对允许重试的错误码（`PROVIDER.UNAVAILABLE` 等），在指数退避后第二次成功；否则违例。

------

## 4. 适配器（`adapters/*`）

> 适配器负责把 Case.input 转成被测系统的调用，并返回**公共视图响应**（PublicResp）。
>  适配器不得透出敏感字段，错误码必须映射为**稳定码**（SB-02）。

### 4.1 通用接口（`adapters/mod.rs`）

```rust
#[async_trait::async_trait]
pub trait SutAdapter: Send + Sync {
  async fn call(&self, ctx: &RunCtx, case: &Case) -> Result<PublicResp, TestkitError>;
}
```

### 4.2 LLM（`adapters/llm.rs`）

- 使用 `soulbase-llm`：`ChatRequest`（注入 `model_alias`、`seed`、`tool_specs` 只读）；
- 错误映射：上下文溢出 → `LLM.CONTEXT_OVERFLOW`；超时 → `LLM.TIMEOUT`；
- 公共视图：`{"text": "..."} | {"json": <obj>}` + `usage/cost（摘要）`。

### 4.3 Tools（`adapters/tools.rs`）

- 使用 `soulbase-tools` + `soulbase-sandbox`：走 **Preflight → Invoke**；
- 公共视图：`{"ok":bool,"output":<schema-ok>}`；
- 错误映射：`SCHEMA.VALIDATION_FAILED | AUTH.FORBIDDEN | SANDBOX.CAPABILITY_BLOCKED` 等。

### 4.4 Storage / Tx / Auth / Http（同理）

- Storage：所有查询**参数化**；
- Tx：Outbox/Saga/Idempo 返回公共视图（不落敏感载荷）；
- Auth：`AuthN/AuthZ` 走 `AuthFacade`，公共视图仅含 `allow/obligations摘要`；
- Http：通过拦截器注入 Envelope，公共视图为**公共错误体**。

------

## 5. Runner（`runner/*`）

### 5.1 执行计划（`plan.rs`）

```rust
pub struct ExecPlan {
  pub matrix: Vec<MatrixKey>,        // 版本×供应商×配置的笛卡尔积（可过滤）
  pub parallel: usize,               // 并发度
  pub max_cases: Option<usize>,      // 采样规模控制
}
```

### 5.2 引擎（`engine.rs`）

```rust
pub struct Runner<A: SutAdapter, S: Asserter> {
  pub adapter: A,
  pub asserters: Vec<S>,             // 可多种断言器并用
  pub headless: bool,                // 影子/沙箱模式（不执行副作用；由适配器负责）
}

impl<A: SutAdapter, S: Asserter> Runner<A,S> {
  pub async fn run(&self, spec: &ContractSpec, cases: &[Case], plan: &ExecPlan) -> RunReport {
    let mut matrix_res = vec![];
    let mut case_reports = vec![];

    for key in &plan.matrix {
      let mut totals=0; let mut passed=0; let mut failed=0; let mut unknown=0;
      let mut by_code = std::collections::BTreeMap::new();

      for case in cases.iter().take(plan.max_cases.unwrap_or(cases.len())) {
        let ctx = RunCtx{ spec: spec.clone(), matrix_key: key.clone() };
        let resp = match self.adapter.call(&ctx, case).await {
          Ok(r) => r,
          Err(e) => PublicResp{ code: Some("UNKNOWN.INTERNAL".into()), http_status: None, body: serde_json::json!({"error":e.to_string()}), events: vec![] }
        };
        // 聚合断言
        let mut violations = vec![];
        for asserter in &self.asserters {
          violations.extend(asserter.assert(&ctx, case, &resp).await);
        }
        // Clause 断言（Case 自带）
        for cl in &case.expect {
          match cl {
            Clause::SchemaOk => {/* 已由 SchemaAsserter 做；此处不重复 */}
            Clause::ErrorIs{code} => if resp.code.as_deref() != Some(code.as_str()) { violations.push(format!("ERROR!= {}", code)); }
            Clause::FieldEq{path,value} => {
              if resp.body.pointer(path.as_str()).unwrap_or(&serde_json::Value::Null) != value { violations.push(format!("FIELD {path} != expected")); }
            }
            Clause::InvariantHold{..} => {/* 已由 InvariantAsserter */}
          }
        }
        let ok = violations.is_empty();
        totals += 1; if ok { passed += 1; } else { failed += 1; }
        if resp.code.as_deref().unwrap_or("").starts_with("UNKNOWN") { unknown += 1; }
        if let Some(c) = resp.code { *by_code.entry(c).or_insert(0) += 1; }

        case_reports.push(CaseReport{ case_id: case.id.clone(), ok, code: resp.body.get("code").and_then(|v| v.as_str()).map(|s| s.to_string()).or(None), violations });
      }
      matrix_res.push(MatrixResult{ key: key.clone(), pass: failed==0 && unknown==0, totals, passed, failed, unknown, by_code });
    }
    RunReport{
      spec_id: spec.id.clone(),
      matrix: matrix_res,
      cases: case_reports,
      summary: Summary{ totals: cases.len() as u32, passed: cases.len() as u32, failed: 0, unknown: 0 }
    }
  }
}
```

> 与 **SB-11 observe** 集成：在 `Runner` 内部可选采集 `contract_cases_total{module,pass}`、`contract_errors_total{code}` 等指标，并将每次断言的 `Evidence<ContractEvent>` 发送到 EvidenceBus（RIS 再实现）。

------

## 6. 报告生成与 Diff（`io/report_json.rs`, `io/report_console.rs`）

- **JSON 报告**：直接序列化 `RunReport`；
- **控制台摘要**：
  - 打印矩阵通过谱（version×provider×config）表格；
  - 列出失败 Case 的 `violations` 与 `code` 分布；
  - `UNKNOWN.*` 出现即高亮；
- CI Gate：若 `failed>0 || unknown>0` → 退出码非 0，并输出**第一条违例详情**与**如何复现**（Case id）。

------

## 7. 与 SB-11/SB-12 的协作

- **SB-11 observe**：为契约运行统一打点与证据事件；
- **SB-12 benchmark**：同一套 Case 可挂到 Benchmark 作为性能门禁；ContractTestKit 关注**行为/语义**，Benchmark 关注**性能/成本**；
- 两者的报告可在 CI 中**并排展示**，形成“功能 + 性能”的双门禁。

------

## 8. 安全与最小披露

- 适配器返回**公共视图**；任何原始字段应在适配器层截断/脱敏；
- 报告仅记录摘要与稳定码，不落原始敏感数据；
- 当 `strict-redaction` feature 打开时，额外对 `Case.input` 的敏感键执行屏蔽后再保存到报告。

------

## 9. 下一步（RIS 预告）

在 **SB-13-RIS** 中将提供：

- `SchemaAsserter/ErrorAsserter/InvariantAsserter/SecurityAsserter/IdempotencyAsserter` 的最小实现；
- `EchoLlmAdapter/MockToolAdapter/MockStorageAdapter` 适配器；
- `Runner` 的可运行骨架与 2–3 个示例 Spec/Case（LLM 正/负向；Tools 未授权/Schema 失败）；
- JSON 报告与控制台摘要；
- 端到端单测验证：**稳定错误码断言**与**不变式（DenyByDefault/MinimalDisclosure）**生效。
