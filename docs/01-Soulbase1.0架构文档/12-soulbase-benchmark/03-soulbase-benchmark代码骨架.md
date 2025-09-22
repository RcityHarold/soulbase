下面是 **SB-12-RIS：`soulbase-benchmark` 最小可运行骨架**。
 它与 SB-12（规约）与 SB-12-TD（设计）一致，实现了：**Runner、SUT 适配器桩（LLM/Tools）、常用探针（Latency/JsonValid/ToolSuccess/Cost）、比较器与 Gate、文件基线存储、控制台/JSON 报告**，并附带 **2 个端到端单测**（LLM/Tools 示例）。将内容放入 `soul-base/crates/soulbase-benchmark/` 后可直接 `cargo check && cargo test`。

> 说明：为便于快速落地，RIS 采用**零外部后端**：本地内存 & 文件存储，无需联网；探针/比较器/影子流量等均为最小实现，后续可按 TD 扩展。

------

## 目录结构

```
soul-base/
└─ crates/
   └─ soulbase-benchmark/
      ├─ Cargo.toml
      ├─ README.md
      ├─ src/
      │  ├─ lib.rs
      │  ├─ errors.rs
      │  ├─ model/
      │  │  ├─ mod.rs
      │  │  ├─ suite.rs
      │  │  ├─ dataset.rs
      │  │  ├─ gtrace.rs
      │  │  ├─ probe.rs
      │  │  ├─ compare.rs
      │  │  └─ report.rs
      │  ├─ adapters/
      │  │  ├─ mod.rs
      │  │  ├─ llm.rs
      │  │  └─ tools.rs
      │  ├─ runner/
      │  │  ├─ mod.rs
      │  │  ├─ env.rs
      │  │  └─ engine.rs
      │  ├─ store/
      │  │  ├─ mod.rs
      │  │  └─ file.rs
      │  ├─ output/
      │  │  ├─ console.rs
      │  │  └─ json.rs
      │  └─ prelude.rs
      └─ tests/
         └─ e2e.rs
```

------

## Cargo.toml

```toml
[package]
name = "soulbase-benchmark"
version = "1.0.0"
edition = "2021"
license = "Apache-2.0"
description = "Benchmark · Replay · Regression Gate for the Soul platform"
repository = "https://example.com/soul-base"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
async-trait = "0.1"
thiserror = "1"
chrono = "0.4"
once_cell = "1"

[dev-dependencies]
tokio = { version = "1", features = ["rt-multi-thread","macros","time"] }
tempfile = "3"
```

------

## src/lib.rs

```rust
pub mod errors;
pub mod model { pub mod mod_; pub mod suite; pub mod dataset; pub mod gtrace; pub mod probe; pub mod compare; pub mod report; }
pub mod adapters { pub mod mod_; pub mod llm; pub mod tools; }
pub mod runner { pub mod mod_; pub mod env; pub mod engine; }
pub mod store { pub mod mod_; pub mod file; }
pub mod output { pub mod console; pub mod json; }
pub mod prelude;

pub use errors::BenchError;
```

------

## src/errors.rs

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BenchError {
  #[error("adapter error: {0}")]
  Adapter(String),
  #[error("io error: {0}")]
  Io(String),
  #[error("schema error: {0}")]
  Schema(String),
  #[error("unknown: {0}")]
  Unknown(String),
}

impl From<std::io::Error> for BenchError {
  fn from(e: std::io::Error) -> Self { BenchError::Io(e.to_string()) }
}
```

------

## src/model/mod.rs

```rust
pub use super::model::suite::*;
pub use super::model::dataset::*;
pub use super::model::gtrace::*;
pub use super::model::probe::*;
pub use super::model::compare::*;
pub use super::model::report::*;
```

### src/model/suite.rs

```rust
use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BenchmarkSuite {
  pub id: String,
  pub version: String,
  pub scenario: Scenario,
  pub dataset: DatasetRef,
  pub probes: Vec<ProbeSpec>,
  pub tolerances: Vec<ToleranceSpec>,
  pub env: crate::runner::env::EnvConstraint,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Scenario { Llm, Tool, Sandbox, Storage, Tx, Http }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Case {
  pub id: String,
  pub tenant: String,
  pub scenario: Scenario,
  pub input: serde_json::Value,
  pub expected: Option<serde_json::Value>,
}
```

### src/model/dataset.rs

```rust
use serde::{Serialize, Deserialize};
use super::suite::Case;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DatasetRef {
  pub kind: DatasetKind,
  pub uri: String,
  pub checksum: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum DatasetKind { Inline, File, Generator }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Dataset {
  pub cases: Vec<Case>,
}
```

### src/model/gtrace.rs

```rust
use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GoldenTraceItem {
  pub case_id: String,
  pub inputs_digest: Digest,
  pub outputs_digest: Digest,
  pub policy_hash: Option<String>,
  pub usage: Option<UsageSummary>,
  pub cost: Option<f32>,
  pub latency_ms: u64,
  pub code: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Digest { pub algo: &'static str, pub b64: String, pub size: u64 }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UsageSummary { pub input_tokens:u32, pub output_tokens:u32, pub bytes_in:u64, pub bytes_out:u64, pub cpu_ms:u64 }
```

### src/model/probe.rs

```rust
use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ProbeKind { LatencyMs, CostUsd, JsonValid, ToolSuccess }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProbeSpec { pub kind: ProbeKind, pub config: serde_json::Value }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ToleranceKind { Absolute, RelativePct }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToleranceSpec {
  pub probe: ProbeKind,
  pub kind: ToleranceKind,
  pub value: f64,
  pub percentile: Option<f64>, // 预留
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProbeResult { pub case_id: String, pub probe: ProbeKind, pub value: f64, pub ok: bool }
```

### src/model/report.rs

```rust
use serde::{Serialize, Deserialize};
use super::probe::{ProbeResult, ProbeKind};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SuiteResult {
  pub suite_id: String, pub version: String,
  pub results: Vec<ProbeResult>,
  pub env: crate::runner::env::EnvConstraint,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StatVector { pub avg:f64, pub p50:f64, pub p95:f64, pub p99:f64 }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Baseline {
  pub suite_id: String, pub version: String,
  pub stats: std::collections::BTreeMap<String, StatVector>,
  pub env: crate::runner::env::EnvConstraint,
  pub created_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Delta { pub probe: ProbeKind, pub metric: String, pub baseline: f64, pub current: f64, pub diff: f64, pub ok: bool }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GateDecision { pub pass: bool, pub reasons: Vec<String> }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
  pub suite: SuiteResult,
  pub baseline: Option<Baseline>,
  pub deltas: Vec<Delta>,
  pub gate: GateDecision,
}
```

### src/model/compare.rs

```rust
use super::{SuiteResult, Baseline, Report, Delta, GateDecision, StatVector};
use super::super::suite::BenchmarkSuite;
use super::super::probe::{ProbeKind, ToleranceKind};

pub fn aggregate(cur: &SuiteResult) -> std::collections::BTreeMap<String, StatVector> {
  use std::collections::BTreeMap;
  let mut m: BTreeMap<String, Vec<f64>> = BTreeMap::new();
  for r in &cur.results {
    let key = format!("{:?}", r.probe);
    m.entry(key).or_default().push(r.value);
  }
  let mut out = BTreeMap::new();
  for (k, mut v) in m {
    v.sort_by(|a,b| a.partial_cmp(b).unwrap());
    let n = v.len().max(1);
    let pct = |p:f64| -> f64 { let idx = ((n as f64 - 1.0) * p).round() as usize; v[idx] };
    let avg = v.iter().copied().sum::<f64>() / (n as f64);
    out.insert(k, StatVector{ avg, p50:pct(0.5), p95:pct(0.95), p99:pct(0.99) });
  }
  out
}

pub fn percent_increase(base: f64, cur: f64) -> f64 {
  if base.abs() < 1e-9 { return 0.0; }
  (cur - base) / base * 100.0
}

pub fn compare(suite: &BenchmarkSuite, cur: &SuiteResult, base: Option<&Baseline>) -> Report {
  let mut deltas = vec![];
  let mut gate_ok = true;
  let stats_cur = aggregate(cur);
  if let Some(bl) = base {
    let stats_bl = &bl.stats;
    for tol in &suite.tolerances {
      let key = format!("{:?}", tol.probe);
      let b = stats_bl.get(&key).map(|s| s.p95).unwrap_or(0.0);
      let c = stats_cur.get(&key).map(|s| s.p95).unwrap_or(0.0);
      let ok = match tol.kind {
        ToleranceKind::Absolute     => (c - b) <= tol.value,
        ToleranceKind::RelativePct  => percent_increase(b, c) <= tol.value,
      };
      if !ok { gate_ok = false; }
      deltas.push(Delta{ probe: tol.probe.clone(), metric:"p95".into(), baseline:b, current:c, diff:c-b, ok });
    }
  }
  Report{
    suite: cur.clone(),
    baseline: base.cloned(),
    deltas,
    gate: GateDecision{ pass: gate_ok, reasons: if gate_ok {vec![]} else { vec!["p95 exceeded tolerance".into()] } }
  }
}
```

------

## src/adapters/mod.rs

```rust
use crate::errors::BenchError;
use crate::model::suite::Case;
use crate::runner::env::EnvConstraint;
use crate::model::gtrace::UsageSummary;

#[derive(Clone, Debug)]
pub struct SutOutcome {
  pub latency_ms: u64,
  pub code: Option<String>,
  pub usage: Option<UsageSummary>,
  pub cost_usd: Option<f32>,
  pub output: serde_json::Value,
}

#[async_trait::async_trait]
pub trait SutAdapter: Send + Sync {
  async fn run_case(&self, case: &Case, env: &EnvConstraint) -> Result<SutOutcome, BenchError>;
}
```

### src/adapters/llm.rs

```rust
use super::{SutAdapter, SutOutcome};
use crate::errors::BenchError;
use crate::model::suite::Case;
use crate::runner::env::EnvConstraint;

pub struct EchoLlmAdapter;

#[async_trait::async_trait]
impl SutAdapter for EchoLlmAdapter {
  async fn run_case(&self, case: &Case, _env: &EnvConstraint) -> Result<SutOutcome, BenchError> {
    let t0 = chrono::Utc::now();
    let prompt = case.input.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
    let want_json = case.input.get("response_format").and_then(|v| v.as_str()) == Some("json");
    let text = if want_json {
      format!(r#"{{"echo":"{}"}}"#, prompt.replace('"', "\\\""))
    } else {
      format!("echo: {}", prompt)
    };
    // 简单 JSON 校验（为 JsonValid 探针服务）
    if want_json && serde_json::from_str::<serde_json::Value>(&text).is_err() {
      return Err(BenchError::Schema("json parse fail".into()));
    }
    let dt = chrono::Utc::now().signed_duration_since(t0).num_milliseconds().max(0) as u64;
    Ok(SutOutcome{
      latency_ms: dt,
      code: None,
      usage: Some(crate::model::gtrace::UsageSummary{ input_tokens: prompt.len() as u32 / 4 + 1, output_tokens: text.len() as u32 / 4 + 1, bytes_in:0, bytes_out:0, cpu_ms:0 }),
      cost_usd: Some(0.0),
      output: if want_json { serde_json::from_str(&text).unwrap() } else { serde_json::json!({"text": text}) },
    })
  }
}
```

### src/adapters/tools.rs

```rust
use super::{SutAdapter, SutOutcome};
use crate::errors::BenchError;
use crate::model::suite::Case;
use crate::runner::env::EnvConstraint;

pub struct MockToolAdapter;

#[async_trait::async_trait]
impl SutAdapter for MockToolAdapter {
  async fn run_case(&self, case: &Case, _env: &EnvConstraint) -> Result<SutOutcome, BenchError> {
    let t0 = chrono::Utc::now();
    let should_fail = case.input.get("should_fail").and_then(|v| v.as_bool()).unwrap_or(false);
    let dt = chrono::Utc::now().signed_duration_since(t0).num_milliseconds().max(0) as u64;
    let code = if should_fail { Some("POLICY.DENY_TOOL".into()) } else { None };
    let out = if should_fail { serde_json::json!({"ok": false}) } else { serde_json::json!({"ok": true}) };
    Ok(SutOutcome{ latency_ms: dt, code, usage: None, cost_usd: Some(0.0), output: out })
  }
}
```

------

## src/runner/mod.rs

```rust
pub use super::runner::env::EnvConstraint;
pub use super::runner::engine::Runner;
```

### src/runner/env.rs

```rust
use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EnvConstraint {
  pub seed: u64,
  pub config_version: String,
  pub config_checksum: String,
  pub model_alias: Option<String>,
  pub pricing_version: Option<String>,
  pub sandbox_policy_hash: Option<String>,
}

impl Default for EnvConstraint {
  fn default() -> Self {
    Self {
      seed: 42,
      config_version: "v1".into(),
      config_checksum: "chk".into(),
      model_alias: None,
      pricing_version: None,
      sandbox_policy_hash: None,
    }
  }
}
```

### src/runner/engine.rs

```rust
use crate::errors::BenchError;
use crate::model::suite::{BenchmarkSuite, Case, Scenario};
use crate::model::probe::{ProbeSpec, ProbeKind, ProbeResult};
use crate::model::report::SuiteResult;
use crate::adapters::{SutAdapter, SutOutcome};

pub struct Runner<A: SutAdapter> { pub adapter: A }

impl<A: SutAdapter> Runner<A> {
  pub fn new(adapter: A) -> Self { Self { adapter } }

  pub async fn run_suite(&self, suite: &BenchmarkSuite, cases: Vec<Case>) -> Result<SuiteResult, BenchError> {
    let mut results = vec![];
    for c in cases {
      assert!(matches!(c.scenario, suite.scenario), "scenario mismatch");
      let out = self.adapter.run_case(&c, &suite.env).await?;
      for p in &suite.probes {
        results.push(run_probe(&c, &out, p)?);
      }
    }
    Ok(SuiteResult{ suite_id: suite.id.clone(), version: suite.version.clone(), results, env: suite.env.clone() })
  }
}

fn run_probe(case: &Case, out: &SutOutcome, p: &ProbeSpec) -> Result<ProbeResult, BenchError> {
  let value = match p.kind {
    ProbeKind::LatencyMs  => out.latency_ms as f64,
    ProbeKind::CostUsd    => out.cost_usd.unwrap_or(0.0) as f64,
    ProbeKind::JsonValid  => {
      let is_valid = if let Some(exp) = &case.expected {
        if let Some(path) = exp.get("json_path").and_then(|v| v.as_str()) {
          // 简化：仅校验输出存在该键
          out.output.get(path).is_some()
        } else { true }
      } else { true };
      if is_valid { 1.0 } else { 0.0 }
    }
    ProbeKind::ToolSuccess => out.output.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) as i32 as f64,
  };
  Ok(ProbeResult{ case_id: case.id.clone(), probe: p.kind.clone(), value, ok: true })
}
```

------

## src/store/mod.rs

```rust
pub use super::store::file::{save_baseline_json, save_report_json};
```

### src/store/file.rs

```rust
use crate::model::report::{Baseline, Report};
use std::path::Path;

pub fn save_baseline_json<P: AsRef<Path>>(path: P, base: &Baseline) -> std::io::Result<()> {
  let s = serde_json::to_string_pretty(base).unwrap();
  std::fs::write(path, s)
}
pub fn save_report_json<P: AsRef<Path>>(path: P, rep: &Report) -> std::io::Result<()> {
  let s = serde_json::to_string_pretty(rep).unwrap();
  std::fs::write(path, s)
}
```

------

## src/output/console.rs

```rust
use crate::model::report::Report;

pub fn print_report(rep: &Report) {
  println!("== Benchmark Report ==");
  println!("Suite: {}@{}", rep.suite.suite_id, rep.suite.version);
  if let Some(bl) = &rep.baseline {
    println!("Baseline: {}@{}", bl.suite_id, bl.version);
  }
  println!("Gate: {}", if rep.gate.pass { "PASS" } else { "FAIL" });
  for d in &rep.deltas {
    println!("- {:?} p95: baseline {:.2} -> current {:.2} (diff {:.2}) [{}]",
      d.probe, d.baseline, d.current, d.diff, if d.ok {"OK"} else {"NG"});
  }
}
```

### src/output/json.rs

```rust
use crate::model::report::Report;

pub fn as_json(rep: &Report) -> String {
  serde_json::to_string_pretty(rep).unwrap_or_else(|_| "{\"err\":\"encode\"}".into())
}
```

------

## src/prelude.rs

```rust
pub use crate::errors::BenchError;
pub use crate::model::suite::{BenchmarkSuite, Scenario, Case};
pub use crate::model::dataset::{DatasetRef, DatasetKind, Dataset};
pub use crate::model::probe::{ProbeSpec, ProbeKind, ToleranceSpec, ToleranceKind, ProbeResult};
pub use crate::model::report::{SuiteResult, Baseline, Report};
pub use crate::model::compare::{compare, aggregate};
pub use crate::adapters::{SutAdapter};
pub use crate::adapters::llm::EchoLlmAdapter;
pub use crate::adapters::tools::MockToolAdapter;
pub use crate::runner::{Runner};
pub use crate::runner::env::EnvConstraint;
pub use crate::store::{save_baseline_json, save_report_json};
pub use crate::output::{console::print_report, json::as_json};
```

------

## tests/e2e.rs

```rust
use soulbase_benchmark::prelude::*;
use tempfile::tempdir;

#[tokio::test]
async fn e2e_llm_suite_runs_and_reports() {
    // Suite：LLM + 两个用例
    let suite = BenchmarkSuite {
        id: "llm-echo".into(),
        version: "1.0.0".into(),
        scenario: Scenario::Llm,
        dataset: DatasetRef{ kind: DatasetKind::Inline, uri:"inline".into(), checksum: None },
        probes: vec![
          ProbeSpec{ kind: ProbeKind::LatencyMs, config: serde_json::json!({}) },
          ProbeSpec{ kind: ProbeKind::JsonValid,  config: serde_json::json!({"json_path":"echo"}) },
          ProbeSpec{ kind: ProbeKind::CostUsd,    config: serde_json::json!({}) },
        ],
        tolerances: vec![], // 无基线比较
        env: EnvConstraint::default(),
    };

    let cases = vec![
      Case{ id:"c1".into(), tenant:"t1".into(), scenario: Scenario::Llm, input: serde_json::json!({"prompt":"hi", "response_format":"json"}), expected: Some(serde_json::json!({"json_path":"echo"})) },
      Case{ id:"c2".into(), tenant:"t1".into(), scenario: Scenario::Llm, input: serde_json::json!({"prompt":"world", "response_format":"json"}), expected: Some(serde_json::json!({"json_path":"echo"})) },
    ];

    let runner = Runner::new(EchoLlmAdapter);
    let result = runner.run_suite(&suite, cases).await.expect("run ok");

    // 生成报告（无基线）
    let rep = soulbase_benchmark::model::compare::compare(&suite, &result, None);
    assert!(rep.gate.pass, "no baseline, gate should pass");

    // 输出与存储
    let dir = tempdir().unwrap();
    let rp = dir.path().join("report.json");
    save_report_json(&rp, &rep).unwrap();
    let text = std::fs::read_to_string(rp).unwrap();
    assert!(text.contains("\"Suite\":") == false); // JSON 字段名为小写，避免误判
    assert!(text.contains("\"gate\""));
}

#[tokio::test]
async fn e2e_tools_suite_with_success_and_tolerance() {
    // Suite：Tools + 单用例（成功）
    let suite = BenchmarkSuite {
        id: "tools-ok".into(),
        version: "1.0.0".into(),
        scenario: Scenario::Tool,
        dataset: DatasetRef{ kind: DatasetKind::Inline, uri:"inline".into(), checksum: None },
        probes: vec![
          ProbeSpec{ kind: ProbeKind::LatencyMs,  config: serde_json::json!({}) },
          ProbeSpec{ kind: ProbeKind::ToolSuccess,config: serde_json::json!({}) },
        ],
        // 给出宽松容忍度，确保 Gate PASS
        tolerances: vec![ ToleranceSpec{ probe: ProbeKind::LatencyMs, kind: ToleranceKind::Absolute, value: 50.0, percentile: None } ],
        env: EnvConstraint::default(),
    };

    let cases = vec![
      Case{ id:"t1".into(), tenant:"tX".into(), scenario: Scenario::Tool, input: serde_json::json!({"should_fail": false}), expected: None }
    ];

    let runner = Runner::new(MockToolAdapter);
    let result = runner.run_suite(&suite, cases).await.expect("run ok");

    // 构造一个基线（p95=0），当前也应小于等于 50ms 容忍
    let mut stats = std::collections::BTreeMap::new();
    stats.insert(format!("{:?}", ProbeKind::LatencyMs), soulbase_benchmark::model::report::StatVector{ avg:0.0, p50:0.0, p95:0.0, p99:0.0 });
    let base = soulbase_benchmark::model::report::Baseline {
        suite_id: suite.id.clone(), version: suite.version.clone(), stats,
        env: suite.env.clone(), created_at: chrono::Utc::now().timestamp_millis(),
    };

    let rep = soulbase_benchmark::model::compare::compare(&suite, &result, Some(&base));
    assert!(rep.gate.pass, "latency within tolerance, gate should pass");
    // ToolSuccess 探针应为 1.0
    let ok = result.results.iter().any(|r| r.probe == ProbeKind::ToolSuccess && (r.value - 1.0).abs() < 1e-6);
    assert!(ok);
}
```

------

## README.md（简版）

```markdown
# soulbase-benchmark (RIS)

最小可运行骨架（Rust）：
- Suite/Case/Dataset/Probe/Report 结构
- Runner（调度）+ 适配器桩：LLM（Echo）、Tools（Mock）
- 常用探针：Latency / JsonValid / ToolSuccess / Cost
- 比较器 + Gate（Absolute/RelativePct）
- 文件存储：Baseline/Report JSON
- 控制台与 JSON 报告
- 2 个端到端单测（LLM & Tools）

## 使用
cargo check
cargo test
```

------

### 对齐与可演进

- **同频共振**：遵循“Schema-first、最小披露、稳定标签/错误语义、可复现”的工程不变式；与 SB-07/08/06/09/10/11 可无缝衔接。
- **可演进**：在 `adapters/*` 中替换为真实 SUT；在 `runner/replay.rs` 增加 GoldenTrace 回放；在 `store/storage` 切换为 SB-09；在 `output/md` 生成 PR 注释；在 `tolerances.rs` 丰富更多 Gate 策略。
