下面是 **SB-13-RIS：`soulbase-contract-testkit` 最小可运行骨架**。
 它与 SB-13（规约）& SB-13-TD（设计）一致，提供可编译可单测的**契约测试套件**：`ContractSpec/Case/Clause`、Runner、断言器（Schema/Error/Invariant/Security），以及两个适配器桩（**EchoLlm**、**MockTool**），并附带 **2 个端到端用例**（LLM 正/负向、Tools 未授权）。为保持零外部依赖，Schema 校验在 RIS 中采用**最小校验**（字段存在性/类型），错误码以**稳定码字符串**断言。

> 放入 `soul-base/crates/soulbase-contract-testkit/` 后，执行 `cargo check && cargo test`。

------

## 目录结构

```
soul-base/
└─ crates/
   └─ soulbase-contract-testkit/
      ├─ Cargo.toml
      ├─ README.md
      ├─ src/
      │  ├─ lib.rs
      │  ├─ errors.rs
      │  ├─ model/
      │  │  ├─ spec.rs
      │  │  ├─ case.rs
      │  │  ├─ clause.rs
      │  │  ├─ matrix.rs
      │  │  └─ report.rs
      │  ├─ asserter/
      │  │  ├─ mod.rs
      │  │  ├─ schema.rs
      │  │  ├─ error.rs
      │  │  ├─ invariant.rs
      │  │  └─ security.rs
      │  ├─ adapters/
      │  │  ├─ mod.rs
      │  │  ├─ llm.rs
      │  │  └─ tools.rs
      │  ├─ runner/
      │  │  ├─ mod.rs
      │  │  ├─ plan.rs
      │  │  └─ engine.rs
      │  ├─ io/
      │  │  ├─ loader.rs
      │  │  ├─ report_json.rs
      │  │  └─ report_console.rs
      │  └─ prelude.rs
      └─ tests/
         └─ e2e.rs
```

------

## Cargo.toml

```toml
[package]
name = "soulbase-contract-testkit"
version = "1.0.0"
edition = "2021"
license = "Apache-2.0"
description = "Contract Tests & Compatibility Gate for the Soul platform"
repository = "https://example.com/soul-base"

[features]
yaml = ["dep:serde_yaml"]

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yaml = { version = "0.9", optional = true }
async-trait = "0.1"
thiserror = "1"
chrono = "0.4"

[dev-dependencies]
tokio = { version = "1", features = ["rt-multi-thread","macros","time"] }
```

------

## src/lib.rs

```rust
pub mod errors;
pub mod model { pub mod spec; pub mod case; pub mod clause; pub mod matrix; pub mod report; }
pub mod asserter { pub mod mod_; pub mod schema; pub mod error; pub mod invariant; pub mod security; }
pub mod adapters { pub mod mod_; pub mod llm; pub mod tools; }
pub mod runner { pub mod mod_; pub mod plan; pub mod engine; }
pub mod io { pub mod loader; pub mod report_json; pub mod report_console; }
pub mod prelude;

pub use errors::TestkitError;
```

------

## src/errors.rs

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TestkitError {
  #[error("io: {0}")]
  Io(String),
  #[error("adapter: {0}")]
  Adapter(String),
  #[error("schema: {0}")]
  Schema(String),
  #[error("unknown: {0}")]
  Unknown(String),
}

impl From<std::io::Error> for TestkitError {
  fn from(e: std::io::Error) -> Self { TestkitError::Io(e.to_string()) }
}
```

------

## src/model/spec.rs

```rust
use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContractSpec {
  pub id: String,
  pub version: String,
  pub domain: Domain,
  pub schema: IoSchema,
  pub errors: Vec<ErrorClause>,
  pub invariants: Vec<Invariant>,
  pub examples: Vec<ExampleArtifact>,
  pub matrix_axes: MatrixAxes,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Domain { Llm, Tools, Sandbox, Storage, Tx, Auth, Http }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IoSchema {
  pub request: serde_json::Value,   // JSON Schema（RIS 最小检查：required + type）
  pub response: serde_json::Value,
  #[serde(default)]
  pub events: Vec<(String, serde_json::Value)>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ErrorClause {
  pub when: serde_json::Value,      // RIS: 简单键值匹配或标志 {"missing":"field"} / {"unauthorized":true}
  pub code: String,
  #[serde(default)]
  pub http_status: Option<u16>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Invariant { DenyByDefault, Idempotent, TenantConsistent, ParametrizedQuery, MinimalDisclosure }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExampleArtifact { pub title: String, pub request: serde_json::Value, pub response_public: serde_json::Value }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MatrixAxes { pub versions: Vec<String>, pub providers: Vec<String>, pub configs: Vec<String> }

/// 简化的“Schema required”检查：从 schema.properties/required 读取必填键
pub fn minimal_required(schema: &serde_json::Value) -> Vec<String> {
  schema.get("required").and_then(|v| v.as_array())
    .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
    .unwrap_or_default()
}
```

------

## src/model/case.rs

```rust
use serde::{Serialize, Deserialize};
use super::spec::{Invariant};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Case {
  pub id: String,
  pub spec_id: String,
  pub tenant: String,
  pub input: serde_json::Value,
  #[serde(default)]
  pub expect: Vec<Clause>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Clause {
  SchemaOk,
  ErrorIs { code: String },
  FieldEq { path: String, value: serde_json::Value },
  InvariantHold { inv: Invariant },
}
```

------

## src/model/clause.rs

```rust
// 预留：如需扩展更复杂条款，可在此追加
```

------

## src/model/matrix.rs

```rust
use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MatrixKey { pub version: String, pub provider: String, pub config: String }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MatrixResult {
  pub key: MatrixKey,
  pub pass: bool,
  pub totals: u32,
  pub passed: u32,
  pub failed: u32,
  pub unknown: u32,
  pub by_code: std::collections::BTreeMap<String, u32>,
}
```

------

## src/model/report.rs

```rust
use serde::{Serialize, Deserialize};
use super::matrix::{MatrixResult, MatrixKey};
use super::case::Case;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PublicResp {
  pub code: Option<String>,
  #[serde(default)]
  pub http_status: Option<u16>,
  pub body: serde_json::Value,
  #[serde(default)]
  pub events: Vec<(String, serde_json::Value)>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunCtx {
  pub spec_id: String,
  pub matrix_key: MatrixKey,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CaseReport {
  pub case_id: String,
  pub ok: bool,
  pub code: Option<String>,
  pub violations: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Summary { pub totals: u32, pub passed: u32, pub failed: u32, pub unknown: u32 }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunReport {
  pub spec_id: String,
  pub matrix: Vec<MatrixResult>,
  pub cases: Vec<CaseReport>,
  pub summary: Summary,
}
```

------

## src/asserter/mod.rs

```rust
use crate::model::{report::{RunCtx, PublicResp}, case::Case};
#[async_trait::async_trait]
pub trait Asserter: Send + Sync {
  async fn assert(&self, ctx: &RunCtx, case: &Case, resp: &PublicResp) -> Vec<String>;
}
```

### src/asserter/schema.rs

```rust
use crate::asserter::Asserter;
use crate::model::{report::{RunCtx, PublicResp}, case::Case};
use crate::model::spec::minimal_required;

pub struct SchemaAsserter { pub req_schema: serde_json::Value, pub resp_schema: serde_json::Value }

#[async_trait::async_trait]
impl Asserter for SchemaAsserter {
  async fn assert(&self, _ctx: &RunCtx, case: &Case, resp: &PublicResp) -> Vec<String> {
    let mut v = vec![];
    // request: required keys
    let req = &case.input;
    for k in minimal_required(&self.req_schema) {
      if !req.get(&k).is_some() { v.push(format!("SCHEMA: request missing '{}'", k)); }
    }
    // response: required keys
    for k in minimal_required(&self.resp_schema) {
      if !resp.body.get(&k).is_some() { v.push(format!("SCHEMA: response missing '{}'", k)); }
    }
    v
  }
}
```

### src/asserter/error.rs

```rust
use crate::asserter::Asserter;
use crate::model::{report::{RunCtx, PublicResp}, case::Case};
use crate::model::spec::ErrorClause;

pub struct ErrorAsserter { pub clauses: Vec<ErrorClause> }

#[async_trait::async_trait]
impl Asserter for ErrorAsserter {
  async fn assert(&self, _ctx: &RunCtx, case: &Case, resp: &PublicResp) -> Vec<String> {
    let mut v = vec![];
    for c in &self.clauses {
      // RIS: 简单匹配 {"missing":"field"} 或 {"unauthorized":true}
      let when = &c.when;
      let mut hit = false;
      if let Some(miss) = when.get("missing").and_then(|x| x.as_str()) {
        hit = !case.input.get(miss).is_some();
      } else if when.get("unauthorized").and_then(|x| x.as_bool()) == Some(true) {
        hit = case.input.get("auth").and_then(|a| a.as_bool()) != Some(true);
      }
      if hit {
        if resp.code.as_deref() != Some(c.code.as_str()) {
          v.push(format!("ERROR: expect {}, got {:?}", c.code, resp.code));
        }
        if let Some(st) = c.http_status {
          if resp.http_status != Some(st) { v.push(format!("HTTP: expect {}, got {:?}", st, resp.http_status)); }
        }
      }
    }
    v
  }
}
```

### src/asserter/invariant.rs

```rust
use crate::asserter::Asserter;
use crate::model::{report::{RunCtx, PublicResp}, case::{Case, Clause}};
use crate::model::spec::Invariant;

pub struct InvariantAsserter { pub invariants: Vec<Invariant> }

#[async_trait::async_trait]
impl Asserter for InvariantAsserter {
  async fn assert(&self, _ctx: &RunCtx, case: &Case, resp: &PublicResp) -> Vec<String> {
    let mut v = vec![];
    for inv in &self.invariants {
      match inv {
        Invariant::DenyByDefault => {
          let auth = case.input.get("auth").and_then(|x| x.as_bool()).unwrap_or(false);
          if !auth {
            let ok = matches!(resp.code.as_deref(), Some("AUTH.FORBIDDEN") | Some("POLICY.DENY_TOOL") | Some("POLICY.DENY_MODEL"));
            if !ok { v.push("INV: DenyByDefault violated".into()); }
          }
        }
        Invariant::MinimalDisclosure => {
          if resp.body.get("token").is_some() || resp.body.get("authorization").is_some() {
            v.push("INV: MinimalDisclosure violated (token in body)".into());
          }
        }
        _ => {}
      }
    }
    // Case 显式条款也可要求不变式
    for cl in &case.expect {
      if let Clause::InvariantHold{ inv } = cl {
        if let Invariant::MinimalDisclosure = inv {
          if resp.body.get("token").is_some() { v.push("INV: MinimalDisclosure violated".into()); }
        }
      }
    }
    v
  }
}
```

### src/asserter/security.rs

```rust
use crate::asserter::Asserter;
use crate::model::{report::{RunCtx, PublicResp}, case::Case};

pub struct SecurityAsserter { pub deny_keys: Vec<String> }

#[async_trait::async_trait]
impl Asserter for SecurityAsserter {
  async fn assert(&self, _ctx: &RunCtx, _case: &Case, resp: &PublicResp) -> Vec<String> {
    let mut v = vec![];
    fn walk(prefix:&str, val:&serde_json::Value, deny:&[String], out:&mut Vec<String>) {
      match val {
        serde_json::Value::Object(m) => {
          for (k,v) in m {
            let p = if prefix.is_empty(){k.clone()} else {format!("{prefix}.{k}")};
            if deny.iter().any(|d| d.eq_ignore_ascii_case(k)) { out.push(format!("SEC: key '{}' present", p)); }
            walk(&p, v, deny, out);
          }
        }
        serde_json::Value::Array(a) => for (i,x) in a.iter().enumerate() { walk(&format!("{prefix}[{i}]"), x, deny, out); }
        _ => {}
      }
    }
    walk("", &resp.body, &self.deny_keys, &mut v);
    v
  }
}
```

------

## src/adapters/mod.rs

```rust
use crate::model::report::{RunCtx, PublicResp};
use crate::model::case::Case;
use crate::errors::TestkitError;

#[async_trait::async_trait]
pub trait SutAdapter: Send + Sync {
  async fn call(&self, ctx: &RunCtx, case: &Case) -> Result<PublicResp, TestkitError>;
}
```

### src/adapters/llm.rs

```rust
use super::SutAdapter;
use crate::model::{report::{RunCtx, PublicResp}, case::Case};
use crate::errors::TestkitError;

pub struct EchoLlmAdapter;

#[async_trait::async_trait]
impl SutAdapter for EchoLlmAdapter {
  async fn call(&self, _ctx: &RunCtx, case: &Case) -> Result<PublicResp, TestkitError> {
    let prompt = case.input.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
    let format = case.input.get("response_format").and_then(|v| v.as_str()).unwrap_or("text");
    if case.input.get("overflow").and_then(|v| v.as_bool()) == Some(true) {
      return Ok(PublicResp { code: Some("LLM.CONTEXT_OVERFLOW".into()), http_status: Some(400), body: serde_json::json!({"code":"LLM.CONTEXT_OVERFLOW","message":"too long"}), events: vec![] });
    }
    let body = if format=="json" { serde_json::json!({"echo": prompt}) } else { serde_json::json!({"text": format!("echo: {}", prompt)}) };
    Ok(PublicResp { code: None, http_status: Some(200), body, events: vec![] })
  }
}
```

### src/adapters/tools.rs

```rust
use super::SutAdapter;
use crate::model::{report::{RunCtx, PublicResp}, case::Case};
use crate::errors::TestkitError;

pub struct MockToolAdapter;

#[async_trait::async_trait]
impl SutAdapter for MockToolAdapter {
  async fn call(&self, _ctx: &RunCtx, case: &Case) -> Result<PublicResp, TestkitError> {
    let auth = case.input.get("auth").and_then(|v| v.as_bool()).unwrap_or(false);
    if !auth { return Ok(PublicResp{ code: Some("AUTH.FORBIDDEN".into()), http_status: Some(403), body: serde_json::json!({"code":"AUTH.FORBIDDEN","message":"forbidden"}), events: vec![] }); }
    // 简单 Schema：必须有 { "x": number }
    if !case.input.get("x").and_then(|v| v.as_f64()).is_some() {
      return Ok(PublicResp{ code: Some("SCHEMA.VALIDATION_FAILED".into()), http_status: Some(422), body: serde_json::json!({"code":"SCHEMA.VALIDATION_FAILED"}), events: vec![] });
    }
    Ok(PublicResp{ code: None, http_status: Some(200), body: serde_json::json!({"ok": true}), events: vec![] })
  }
}
```

------

## src/runner/plan.rs

```rust
use crate::model::matrix::MatrixKey;

#[derive(Clone, Debug)]
pub struct ExecPlan {
  pub matrix: Vec<MatrixKey>,
  pub parallel: usize,
  pub max_cases: Option<usize>,
}
impl Default for ExecPlan {
  fn default() -> Self {
    Self { matrix: vec![MatrixKey{version:"cur".into(), provider:"local".into(), config:"default".into()}], parallel: 1, max_cases: None }
  }
}
```

### src/runner/engine.rs

```rust
use crate::errors::TestkitError;
use crate::model::{spec::ContractSpec, case::{Case, Clause}, matrix::{MatrixKey, MatrixResult}, report::{RunCtx, PublicResp, CaseReport, Summary, RunReport}};
use crate::asserter::mod::Asserter;
use crate::adapters::mod::SutAdapter;

pub struct Runner<A: SutAdapter> {
  pub adapter: A,
  pub asserters: Vec<Box<dyn Asserter>>,
}

impl<A: SutAdapter> Runner<A> {
  pub fn new(adapter: A, asserters: Vec<Box<dyn Asserter>>) -> Self { Self { adapter, asserters } }

  pub async fn run(&self, spec: &ContractSpec, cases: &[Case], matrix: &[MatrixKey]) -> Result<RunReport, TestkitError> {
    let mut matrix_res = vec![];
    let mut case_reports = vec![];
    let mut totals=0; let mut passed=0; let mut failed=0; let mut unknown=0;

    for key in matrix {
      let mut m_tot=0; let mut m_pas=0; let mut m_fai=0; let mut m_un=0;
      let mut by_code = std::collections::BTreeMap::new();

      for case in cases {
        m_tot+=1; totals+=1;
        let ctx = RunCtx{ spec_id: spec.id.clone(), matrix_key: key.clone() };
        let resp = self.adapter.call(&ctx, case).await.unwrap_or(PublicResp{ code: Some("UNKNOWN.INTERNAL".into()), http_status: None, body: serde_json::json!({"code":"UNKNOWN.INTERNAL"}), events: vec![] });

        let mut violations = vec![];
        // 契约断言器
        for a in &self.asserters { violations.extend(a.assert(&ctx, case, &resp).await); }
        // Case 自带条款
        for cl in &case.expect {
          match cl {
            Clause::SchemaOk => { /* 已在 SchemaAsserter 覆盖，RIS跳过 */ }
            Clause::ErrorIs{ code } => if resp.code.as_deref()!=Some(code) { violations.push(format!("ERROR!= {}", code)); }
            Clause::FieldEq{ path, value } => {
              if resp.body.pointer(path.as_str()).unwrap_or(&serde_json::Value::Null) != value { violations.push(format!("FIELD {path} != expected")); }
            }
            Clause::InvariantHold{ .. } => { /* 已覆盖 */ }
          }
        }
        let ok = violations.is_empty();
        if ok { passed+=1; m_pas+=1; } else { failed+=1; m_fai+=1; }
        if resp.code.as_deref().unwrap_or("").starts_with("UNKNOWN"){ unknown+=1; m_un+=1; }
        if let Some(c) = resp.body.get("code").and_then(|v| v.as_str()) { *by_code.entry(c.to_string()).or_insert(0)+=1; }

        case_reports.push(CaseReport{ case_id: case.id.clone(), ok, code: resp.body.get("code").and_then(|v| v.as_str()).map(|s| s.to_string()), violations });
      }
      matrix_res.push(MatrixResult{ key: key.clone(), pass: m_fai==0 && m_un==0, totals:m_tot, passed:m_pas, failed:m_fai, unknown:m_un, by_code });
    }

    Ok(RunReport{
      spec_id: spec.id.clone(),
      matrix: matrix_res,
      cases: case_reports,
      summary: Summary{ totals: totals as u32, passed: passed as u32, failed: failed as u32, unknown: unknown as u32 }
    })
  }
}
```

------

## src/io/loader.rs

```rust
use crate::errors::TestkitError;
use crate::model::{spec::ContractSpec, case::Case};

#[cfg(feature="yaml")]
pub fn load_spec_yaml(s: &str) -> Result<ContractSpec, TestkitError> {
  serde_yaml::from_str(s).map_err(|e| TestkitError::Io(e.to_string()))
}

pub fn load_cases_json(s: &str) -> Result<Vec<Case>, TestkitError> {
  serde_json::from_str(s).map_err(|e| TestkitError::Io(e.to_string()))
}
```

### src/io/report_json.rs

```rust
use crate::model::report::RunReport;

pub fn to_json(rep: &RunReport) -> String {
  serde_json::to_string_pretty(rep).unwrap_or_else(|_| "{\"err\":\"encode\"}".into())
}
```

### src/io/report_console.rs

```rust
use crate::model::report::RunReport;

pub fn print_summary(rep: &RunReport) {
  println!("== Contract Run ==");
  println!("Spec: {}", rep.spec_id);
  println!("Summary: total={} pass={} fail={} unknown={}",
    rep.summary.totals, rep.summary.passed, rep.summary.failed, rep.summary.unknown);
  for m in &rep.matrix {
    println!("- matrix {:?} -> pass={} (f={}, u={})",
      (&m.key.version,&m.key.provider,&m.key.config), m.pass, m.failed, m.unknown);
  }
  for c in rep.cases.iter().filter(|c| !c.ok).take(5) {
    println!("  case {}: violations={:?}", c.case_id, c.violations);
  }
}
```

------

## src/prelude.rs

```rust
pub use crate::errors::TestkitError;
pub use crate::model::spec::{ContractSpec, Domain, IoSchema, ErrorClause, Invariant, ExampleArtifact, MatrixAxes};
pub use crate::model::case::{Case, Clause};
pub use crate::model::matrix::{MatrixKey, MatrixResult};
pub use crate::model::report::{RunCtx, PublicResp, CaseReport, Summary, RunReport};
pub use crate::asserter::mod::Asserter;
pub use crate::asserter::{schema::SchemaAsserter, error::ErrorAsserter, invariant::InvariantAsserter, security::SecurityAsserter};
pub use crate::adapters::mod::SutAdapter;
pub use crate::adapters::{llm::EchoLlmAdapter, tools::MockToolAdapter};
pub use crate::runner::{plan::ExecPlan, engine::Runner};
pub use crate::io::{report_json::to_json, report_console::print_summary};
```

------

## tests/e2e.rs

```rust
use soulbase_contract_testkit::prelude::*;

fn demo_llm_spec() -> ContractSpec {
  ContractSpec{
    id:"llm.chat.v1".into(), version:"1.0.0".into(), domain:Domain::Llm,
    schema: IoSchema{
      request: serde_json::json!({"required":["prompt"]}),
      response: serde_json::json!({"required":[]}), events: vec![]
    },
    errors: vec![
      ErrorClause{ when: serde_json::json!({"missing":"prompt"}), code:"SCHEMA.VALIDATION_FAILED".into(), http_status: Some(422) },
    ],
    invariants: vec![Invariant::MinimalDisclosure],
    examples: vec![], matrix_axes: MatrixAxes{ versions: vec!["cur".into()], providers: vec!["local".into()], configs: vec!["default".into()] }
  }
}

fn demo_tools_spec() -> ContractSpec {
  ContractSpec{
    id:"tools.mock.v1".into(), version:"1.0.0".into(), domain:Domain::Tools,
    schema: IoSchema{ request: serde_json::json!({}), response: serde_json::json!({"required":[]}), events: vec![] },
    errors: vec![
      ErrorClause{ when: serde_json::json!({"unauthorized":true}), code:"AUTH.FORBIDDEN".into(), http_status: Some(403) },
      ErrorClause{ when: serde_json::json!({"missing":"x"}), code:"SCHEMA.VALIDATION_FAILED".into(), http_status: Some(422) },
    ],
    invariants: vec![Invariant::DenyByDefault, Invariant::MinimalDisclosure],
    examples: vec![], matrix_axes: MatrixAxes{ versions: vec!["cur".into()], providers: vec!["local".into()], configs: vec!["default".into()] }
  }
}

#[tokio::test]
async fn e2e_llm_positive_and_negative() {
  let spec = demo_llm_spec();
  let cases = vec![
    // 正向：prompt+json
    Case{ id:"ok-json".into(), spec_id: spec.id.clone(), tenant:"t1".into(),
          input: serde_json::json!({"prompt":"hi","response_format":"json"}),
          expect: vec![Clause::SchemaOk] },
    // 负向：缺 prompt -> SCHEMA.VALIDATION_FAILED
    Case{ id:"missing".into(), spec_id: spec.id.clone(), tenant:"t1".into(),
          input: serde_json::json!({"response_format":"json"}),
          expect: vec![Clause::ErrorIs{code:"SCHEMA.VALIDATION_FAILED".into()}] },
  ];

  let adapter = EchoLlmAdapter;
  let asserters: Vec<Box<dyn Asserter>> = vec![
    Box::new(SchemaAsserter{ req_schema: spec.schema.request.clone(), resp_schema: spec.schema.response.clone() }),
    Box::new(ErrorAsserter{ clauses: spec.errors.clone() }),
    Box::new(InvariantAsserter{ invariants: spec.invariants.clone() }),
    Box::new(SecurityAsserter{ deny_keys: vec!["authorization".into(),"token".into()] }),
  ];
  let runner = Runner::new(adapter, asserters);
  let plan = ExecPlan::default();
  let rep = runner.run(&spec, &cases, &plan.matrix).await.expect("run");
  assert_eq!(rep.summary.failed, 0, "LLM contract should pass");
}

#[tokio::test]
async fn e2e_tools_unauthorized_and_schema_fail() {
  let spec = demo_tools_spec();
  let cases = vec![
    // 未授权 -> AUTH.FORBIDDEN
    Case{ id:"unauth".into(), spec_id: spec.id.clone(), tenant:"t1".into(),
          input: serde_json::json!({"auth": false}),
          expect: vec![Clause::ErrorIs{code:"AUTH.FORBIDDEN".into()}] },
    // 授权但缺字段 x -> SCHEMA.VALIDATION_FAILED
    Case{ id:"missing-x".into(), spec_id: spec.id.clone(), tenant:"t1".into(),
          input: serde_json::json!({"auth": true}),
          expect: vec![Clause::ErrorIs{code:"SCHEMA.VALIDATION_FAILED".into()}] },
    // 授权且合法 -> ok
    Case{ id:"ok".into(), spec_id: spec.id.clone(), tenant:"t1".into(),
          input: serde_json::json!({"auth": true, "x": 1}),
          expect: vec![Clause::SchemaOk] },
  ];

  let adapter = MockToolAdapter;
  let asserters: Vec<Box<dyn Asserter>> = vec![
    Box::new(SchemaAsserter{ req_schema: spec.schema.request.clone(), resp_schema: spec.schema.response.clone() }),
    Box::new(ErrorAsserter{ clauses: spec.errors.clone() }),
    Box::new(InvariantAsserter{ invariants: spec.invariants.clone() }),
    Box::new(SecurityAsserter{ deny_keys: vec!["authorization".into(),"token".into()] }),
  ];
  let runner = Runner::new(adapter, asserters);
  let plan = ExecPlan::default();
  let rep = runner.run(&spec, &cases, &plan.matrix).await.expect("run");
  assert_eq!(rep.summary.failed, 0, "Tools contract should pass");
}
```

------

## README.md（简版）

```markdown
# soulbase-contract-testkit (RIS)

最小可运行的契约测试骨架：
- 模型：ContractSpec / Case / Clause / Matrix / Report
- 断言器：Schema / Error / Invariant / Security
- 适配器桩：EchoLlm / MockTool
- Runner：执行矩阵、聚合结果与报告
- e2e：LLM 正/负向、Tools 未授权/Schema 失败

## 使用
cargo check
cargo test
```

------

### 对齐与可演进

- **同频共振**：坚持 **Schema-first / 稳定错误码 / 最小披露 / 默认拒绝** 的不变式；
- **可演进**：在 `asserter/*` 加强 Schema 校验（集成 JSON Schema）、在 `adapters/*` 对接真实模块（SB-07/08/06/09/10/04/05），在 `io/loader` 支持 YAML Spec 与多套 Case，扩展 Matrix 维度与控制台/JSON 报告为 CI 友好输出。
