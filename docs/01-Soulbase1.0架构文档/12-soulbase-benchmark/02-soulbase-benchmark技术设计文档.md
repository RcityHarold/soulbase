# 文档 SB-12-TD：`soulbase-benchmark` 技术设计（Benchmark · Replay · Regression Gate）

> 对应规约：SB-12
>  目标：给出 **Suite/Case/Dataset/GoldenTrace/Probe/Comparator/Report** 的结构定义，**回放引擎与沙箱化策略**，**回归门禁算法**与 **CI 集成接口**；与 `soulbase-*` 模块统一标签、稳定错误语义与最小披露原则保持同频。

------

## 1. Crate 结构与 Feature 规划

```
soulbase-benchmark/
  src/
    lib.rs
    errors.rs
    model/                    # Suite/Case/Dataset/GoldenTrace/Probe/Result/Baseline/Report
      mod.rs
      suite.rs
      dataset.rs
      gtrace.rs
      probe.rs
      compare.rs
      report.rs
    adapters/                 # SUT 适配（通过 Trait 解耦于被测实现）
      mod.rs
      llm.rs                  # SB-07 适配（Chat/Stream）
      tools.rs                # SB-08 适配（Invoker 受控执行）
      sandbox.rs              # SB-06 适配（Exec dry-run / tmp root）
      storage.rs              # SB-09 适配（查询/写入影子库）
      tx.rs                   # SB-10 适配（Outbox/Saga/Idempo shadow）
      http.rs                 # SB-05 影子流量（in-process or external）
    runner/                   # 执行引擎
      mod.rs
      env.rs                  # 环境指纹/种子/快照
      replay.rs               # GoldenTrace 回放器
      shadow.rs               # 影子流量分流器（可选）
      engine.rs               # 调度/并发/速率/隔离
    store/                    # 基线/结果持久化（可选，默认 JSON 文件；或接 SB-09）
      mod.rs
      file.rs
      storage.rs              # 通过 soulbase-storage
    output/
      console.rs              # 人类可读摘要
      json.rs                 # 机器可读报告
      md.rs                   # Markdown 摘要（PR/Git 合并注释）
    presets/
      probes.rs               # 通用探针（时延/成本/结构化/成功率…）
      tolerances.rs           # 常用公差模板（绝对/相对/百分位）
    prelude.rs
```

**Features**

- `adapter-llm | -tools | -sandbox | -storage | -tx | -http`：选择被测域
- `store-file`（默认）、`store-storage`（使用 SB-09）
- `shadow`（启用影子流量）
- `strict-redaction`（对回放/报告做更强脱敏）

------

## 2. 数据模型（`model/*`）

### 2.1 Suite / Case / Dataset（`suite.rs`, `dataset.rs`）

```rust
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct BenchmarkSuite {
  pub id: String,
  pub version: String,                     // SemVer；Suite 变更控制
  pub scenario: Scenario,                  // llm|tool|sandbox|storage|tx|http
  pub dataset: DatasetRef,
  pub probes: Vec<ProbeSpec>,              // 要采集的指标组合
  pub tolerances: Vec<ToleranceSpec>,      // Gate 公差矩阵（绑定 probe）
  pub env: EnvConstraint,                  // 模型/价表/配置戳记/随机种子…
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Scenario { Llm, Tool, Sandbox, Storage, Tx, Http }

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DatasetRef {
  pub kind: DatasetKind,                   // File|Inline|Generator
  pub uri: String,                         // path or generator id
  pub checksum: Option<String>,            // sha256（File 时必填）
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum DatasetKind { File, Inline, Generator }

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Case {
  pub id: String,
  pub tenant: String,
  pub scenario: Scenario,
  pub input: serde_json::Value,            // 按适配器期望；Schema 见各适配器
  pub expected: Option<serde_json::Value>, // 可选期望（结构化/金样）
}
```

**EnvConstraint（环境指纹）**（`runner/env.rs`）

```rust
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct EnvConstraint {
  pub seed: u64,
  pub config_version: String,
  pub config_checksum: String,
  pub model_alias: Option<String>,          // LLM
  pub pricing_version: Option<String>,      // 成本价表
  pub sandbox_policy_hash: Option<String>,  // SB-06
}
```

### 2.2 GoldenTrace（`gtrace.rs`）

```rust
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct GoldenTraceItem {
  pub case_id: String,
  pub inputs_digest: Digest,                // hash + size（最小披露）
  pub outputs_digest: Digest,
  pub policy_hash: Option<String>,
  pub usage: Option<UsageSummary>,          // tokens/bytes/cpu_ms…
  pub cost: Option<f32>,                    // USD
  pub latency_ms: u64,
  pub code: Option<String>,                 // 稳定错误码（若有）
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Digest { pub algo: &'static str, pub b64: String, pub size: u64 }

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct UsageSummary { pub input_tokens:u32, pub output_tokens:u32, pub bytes_in:u64, pub bytes_out:u64, pub cpu_ms:u64 }
```

> GoldenTrace 由 `soulbase-observe` 的 Evidence 与指标导出构建（脱敏/归一化）。

### 2.3 Probe / Tolerance / Compare（`probe.rs`, `compare.rs`）

```rust
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum ProbeKind {
  LatencyMs, CostUsd, JsonValid, SchemaValid, Accuracy, ToolSuccess, BudgetBytesIn, BudgetBytesOut, TxRetryCount
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ProbeSpec {
  pub kind: ProbeKind,
  pub config: serde_json::Value,             // 如 accuracy 需要规则/关键字或评分器 id
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum ToleranceKind { Absolute, RelativePct, PercentileDelta }

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ToleranceSpec {
  pub probe: ProbeKind,
  pub kind: ToleranceKind,
  pub value: f64,                             // 绝对值(ms/usd)/相对百分比/百分位允许差
  pub percentile: Option<f64>,                // p95/p99 when PercentileDelta
}
```

### 2.4 结果 / 基线 / 报告（`report.rs`）

```rust
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ProbeResult { pub case_id: String, pub probe: ProbeKind, pub value: f64, pub ok: bool }

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SuiteResult {
  pub suite_id: String, pub version: String,
  pub results: Vec<ProbeResult>,
  pub env: EnvConstraint,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Baseline {
  pub suite_id: String, pub version: String,
  pub stats: std::collections::BTreeMap<String, StatVector>, // probe_key -> {avg,p95,..}
  pub env: EnvConstraint,
  pub created_at: i64,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct StatVector { pub avg:f64, pub p50:f64, pub p95:f64, pub p99:f64 }

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Report {
  pub suite: SuiteResult,
  pub baseline: Option<Baseline>,
  pub deltas: Vec<Delta>,                   // 与基线差
  pub gate: GateDecision,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Delta { pub probe: ProbeKind, pub metric: String, pub baseline: f64, pub current: f64, pub diff: f64, pub ok: bool }

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct GateDecision { pub pass: bool, pub reasons: Vec<String> }
```

------

## 3. 适配器（SUT 接口，`adapters/*`）

> 适配器负责把 **Case.input** 翻译成被测模块的请求，运行后返回**最小摘要**与可供 Probe 使用的原始材料（仅在内存，不落库）。

### 3.1 通用 SUT 接口

```rust
#[async_trait::async_trait]
pub trait SutAdapter: Send + Sync {
  async fn run_case(&self, case: &Case, env: &EnvConstraint) -> Result<SutOutcome, BenchError>;
}

#[derive(Clone, Debug)]
pub struct SutOutcome {
  pub latency_ms: u64,
  pub code: Option<String>,              // 稳定错误码
  pub usage: Option<UsageSummary>,
  pub cost_usd: Option<f32>,
  pub output: serde_json::Value,         // 受控/脱敏后的结构
}
```

### 3.2 LLM 适配（`adapters/llm.rs`）

- 将 `case.input` 解析为 `ChatRequest`（SB-07），注入 `model_alias` 与 `seed`；
- 若 `response_format=json/schema`，在适配器层进行**严格校验**（以便 `SchemaValid` 探针使用）；
- 产出：`latency_ms, usage(token), cost, output(text/json), code(如 LLM.*)`。

### 3.3 Tools 适配（`adapters/tools.rs`）

- 将 `case.input` 映射为 `ToolCall`，走 **Preflight → Sandbox Invoke**（SB-08/SB-06），启用**沙箱 tmp 根**；
- 产出：`latency_ms, BudgetBytes{in/out}, ToolSuccess(true/false), code`。

### 3.4 Sandbox/Storage/Tx/Http 略（同理）

- **Storage**：影子库连接（或 Mock）；只测**读**或写入影子表；
- **Tx**：Outbox/Saga 在内存后端或影子总线运行；
- **Http**：拦截器+影子分流；仅采集时延与错误码。

------

## 4. 回放引擎（`runner/replay.rs`）

```rust
pub struct Replayer<A: SutAdapter> {
  pub adapter: A,
  pub gtrace: Vec<GoldenTraceItem>,
}

impl<A: SutAdapter> Replayer<A> {
  pub async fn replay(&self, env: &EnvConstraint) -> Result<Vec<ReplayResult>, BenchError> {
    let mut out = vec![];
    for gt in &self.gtrace {
      let case = Case{ id: gt.case_id.clone(), tenant:"bench".into(), scenario: Scenario::Llm /* example */, input: serde_json::json!({}), expected: None };
      let res = self.adapter.run_case(&case, env).await?;
      // 校验摘要一致性
      let ok_digest = digest(&res.output) == gt.outputs_digest.b64;
      out.push(ReplayResult{ case_id: gt.case_id.clone(), ok: ok_digest, latency_ms: res.latency_ms });
    }
    Ok(out)
  }
}

pub struct ReplayResult { pub case_id: String, pub ok: bool, pub latency_ms: u64 }
```

**沙箱化策略**

- Tools/Sandbox：强制 `tmp root` 与出站白名单（使用 SB-06 Profile）；
- Storage：影子库 NS/DB；
- Tx：Outbox/Saga 使用内存或测试通道，避免真实下游副作用。

------

## 5. Runner / Engine（`runner/engine.rs`）

```rust
pub struct Runner<A: SutAdapter> {
  pub adapter: A,
  pub meter: soulbase_observe::prelude::InMemoryMeter,
  pub evidence: soulbase_observe::prelude::MemoryEvidence,
}

impl<A: SutAdapter> Runner<A> {
  pub async fn run_suite(&self, suite: &BenchmarkSuite, cases: Vec<Case>) -> Result<SuiteResult, BenchError> {
    let mut results = vec![];
    for c in cases {
      let t0 = now_ms();
      let out = self.adapter.run_case(&c, &suite.env).await?;
      let dt = now_ms() - t0;
      // 指标采集（与 observe 标签最小集一致）
      let mut lbl = btreemap!{"tenant" => c.tenant.clone(), "scenario" => format!("{:?}", c.scenario)};
      self.meter.counter(&mf::HTTP_REQS).inc(&lbl, 1); // 示例
      self.meter.histogram(&mf::HTTP_LAT_MS).observe_ms(&btreemap!{"route_id" => c.id.clone()}, dt as u64);

      // 生成 ProbeResult（按 probes 列表）
      for p in &suite.probes { results.push(run_probe(&c, &out, p)?); }
    }
    Ok(SuiteResult{ suite_id: suite.id.clone(), version: suite.version.clone(), results, env: suite.env.clone() })
  }
}
```

> `run_probe` 位于 `presets/probes.rs`：对 `LatencyMs/CostUsd/JsonValid/SchemaValid/...` 计算值与 `ok` 标志。

------

## 6. 比较器与 Gate（`compare.rs`）

```rust
pub fn compare(suite: &BenchmarkSuite, cur: &SuiteResult, base: Option<&Baseline>) -> Report {
  let mut deltas = vec![];
  let mut gate_ok = true;
  let stats = aggregate(cur); // 计算 avg/p50/p95/p99

  if let Some(bl) = base {
    for tol in &suite.tolerances {
      let key = probe_key(&tol.probe, "p95"); // 例：以 p95 比较
      let b = bl.stats.get(&key).map(|s| s.p95).unwrap_or(0.0);
      let c = stats.get(&key).map(|s| s.p95).unwrap_or(0.0);
      let ok = match tol.kind {
        ToleranceKind::Absolute     => (c - b) <= tol.value,
        ToleranceKind::RelativePct  => percent_increase(b, c) <= tol.value,
        ToleranceKind::PercentileDelta => (c - b) <= tol.value, // 依 percentile 字段扩展
      };
      if !ok { gate_ok = false; }
      deltas.push(Delta{ probe: tol.probe.clone(), metric:"p95".into(), baseline:b, current:c, diff:c-b, ok });
    }
  }
  Report{ suite: cur.clone(), baseline: base.cloned(), deltas, gate: GateDecision{ pass: gate_ok, reasons: if gate_ok {vec![]} else {collect_reasons(&deltas)} } }
}
```

------

## 7. 基线存储与报告输出（`store/*`, `output/*`）

- `store-file`：把 `Baseline`/`Report` 以 JSON 写入 `bench_out/`，文件名含 `suite_id/version/timestamp`；
- `store-storage`：使用 SB-09 Repository（表：`bench_baseline`/`bench_report`），索引 `tenant,suite_id,version,created_at`；
- `output/console`：表格摘要 + 失败列表；
- `output/md`：PR 友好 Markdown；
- `output/json`：机器可读（供 CI 或质控平台消费）。

------

## 8. 与 `soulbase-observe` 的集成

- Runner 在每次 `run_case` 后使用 observe SDK 产出：
  - `bench_latency_ms{scenario,case}` 直方图；
  - `bench_cost_usd{scenario,case}` 计数器；
  - `bench_schema_ok{scenario,case}`/`bench_success{scenario,case}`；
- 统一标签白名单：`tenant,scenario,case,code`；
- 证据：若启用 Golden 回放，生成 `Envelope<BenchReplayEvent>`（只含摘要与比较结果）。

------

## 9. CI 集成接口

- **库 API**：`run_and_gate(suite: BenchmarkSuite) -> Report`；
- **命令行（建议）**：
  - `bench run --suite ./suite.yml --dataset ./dataset.json --baseline ./baseline.json --out ./bench_out`
  - 退出码：`0=PASS / 2=GATE_FAIL / 3=RUNTIME_ERROR`；
- **PR 注释**：输出 `output/md` 摘要；
- **缓存**：可选用 `--reuse-dataset-sha` 与 `--pin-baseline`。

------

## 10. 安全与合规

- 运行时**强制脱敏**：Suite/Case/GoldenTrace 的 `input/output` 在落盘时只保留摘要；
- 影子流量仅走**只读**或**沙箱写**路径；
- 允许在 Suite 上声明 `privacy=high`，此时自动：关闭落盘原始字段 + 加强采样屏蔽。

------

## 11. 测试与验收

- **契约测试**：Suite/Case/GTrace/Report JSON Schema 校验；基线存取幂等；
- **黑盒**：LLM JSON 输出有效率统计、Tools 受控执行成功率、Sandbox 预算统计一致性；
- **回放**：GoldenTrace 回放一致性（摘要一致率 = 100%）；
- **Gate**：在构造的**超阈值**数据上触发 FAIL，并输出明确理由。

------

## 12. 开放问题

- 评分器（accuracy）的**插件协议**（例如基于关键字/正则/LLM 判别器）；
- 多模质量指标（图像/音频）；
- 大规模回放的**分布式执行**与速率控制；
- 与 `soulbase-contract-testkit` 的**联合报告**（功能正确性 + 性能门禁一次性出具）。

------

> 同频总结：本 TD 将“**Suite/Case/Probe → 回放/影子 → 指标与比较 → Gate**”标准化，遵循 **最小披露、稳定标签、可复现** 的底线。若确认无误，我将按“三件套”继续输出 **SB-12-RIS（最小可运行骨架）**，实现最小的 Runner、适配器桩、常用探针与比较器、文件基线存储与控制台/JSON 报告，以及 2–3 个端到端单测（LLM/Tools 示例）。
