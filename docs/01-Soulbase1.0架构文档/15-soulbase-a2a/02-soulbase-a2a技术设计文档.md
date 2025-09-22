# 文档 SB-15-TD：`soulbase-a2a` 技术设计

（Channel · Message · Receipt · Capability/Consent · Sign/Verify · ReplayGuard · Tx/QoS/Observe 集成）

> 对应规约：SB-15
>  目标：给出 **可落地** 的跨域（A2A）技术方案：数据模型、签名/验签 SPI、通道/序列与反重放状态机、能力/同意凭据、请求/回执协议、与 **Tx（可靠投递）/QoS（记账）/Observe（证据与指标）** 的对接、存储结构与热更策略。
>  语言：Rust 接口草案 + SurrealQL 结构；遵循 **稳定错误码 / Envelope 证据 / 最小披露** 不变式。

------

## 1. Crate 结构与 Feature 规划

```
soulbase-a2a/
  src/
    lib.rs
    errors.rs               # A2A.* → soulbase-errors 稳定码
    model/                  # Channel/Peer/Key/Msg/Receipt/Capabilities/Consent
      mod.rs
      ids.rs                # ChannelId/PeerId/Kid/Seq/Nonce
      key.rs                # KeyMaterial/Jwk/Cose 指纹与选择
      peer.rs               # PeerMetadata（DID/域名/策略哈希）
      caps.rs               # CapabilityToken/Scope/Constraints
      consent.rs            # ConsentProof（范围/期限/目的/绑定）
      msg.rs                # A2AMessage Header/Payload/Signature
      receipt.rs            # Receipt(signed) 与收据链
      attn.rs               # Attestation（身份/能力/策略指纹）
    spi/
      signer.rs             # Signer / Verifier / KeyStore（轮换/撤销）
      channel.rs            # ChannelManager / ChannelStore
      replay.rs             # ReplayGuard（seq/nonce/window）
      transport.rs          # 传输抽象（http/bus）
      handler.rs            # InboundPipeline（验证→检查→路由）
      receipt_store.rs      # 收据与证据存储
    facade.rs               # A2AFacade：open/offer/request/notice/receipt
    observe.rs              # 指标/证据事件（A2A*）
    surreal/
      schema.surql          # 表/索引（channel/msg/receipt/nonce）
      repo.rs               # *Store 基于 SB-09 的实现占位
    prelude.rs
```

**Features**

- `sign-jws`（JWS Detached Signature JSON Web Signature）
- `sign-cose`（COSE_Sign1）
- `transport-http` / `transport-bus`
- `surreal`（持久化落地）
- `observe`（SB-11 指标/证据导出）

------

## 2. 数据模型（`model/*`）

### 2.1 标识与序列（`ids.rs`）

```rust
#[derive(Clone, Debug, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ChannelId(pub String);     // "a2a:tenantA~tenantB:uuid"
#[derive(Clone, Debug, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PeerId(pub String);        // "did:example:xyz" 或 "https://peer.example"
#[derive(Clone, Debug, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Kid(pub String);           // key id（JWK kid / COSE kid）
pub type Seq = u64;
pub type Nonce = String;
```

### 2.2 密钥与指纹（`key.rs`）

```rust
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum KeyAlg { Ed25519, ES256, ES384 }
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Jwk { pub kty:String, pub crv:Option<String>, pub x:Option<String>, pub y:Option<String>, pub kid:String } // 最小集
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct KeyMaterial { pub alg: KeyAlg, pub jwk: Jwk, pub not_before: i64, pub not_after: i64, pub fingerprint: String }
```

### 2.3 对端元信息（`peer.rs`）

```rust
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PeerMetadata {
  pub peer_id: PeerId,
  pub endpoint: String,              // http(s) or bus topic
  pub keys: Vec<KeyMaterial>,        // 对端公钥集
  pub policy_hash: String,           // 对端策略快照哈希
  pub pricing_version: Option<String>,
  pub retention_version: Option<String>,
}
```

### 2.4 能力与同意（`caps.rs`, `consent.rs`）

```rust
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Scope { pub resource:String, pub action:String, pub attrs:serde_json::Value } // 与 SB-01/04 对齐

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CapabilityToken {
  pub iss: PeerId,                   // 发行方
  pub sub: PeerId,                   // 授权给
  pub aud: Option<PeerId>,           // 可选受众（对端）
  pub scopes: Vec<Scope>,
  pub nbf: i64, pub exp: i64,
  pub jti: String,                   // 唯一 id
  pub signature: String,             // JWS/COSE（Detached）
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ConsentProof {
  pub subject: String,               // 用户/主体 id
  pub scopes: Vec<Scope>,            // 同意范围
  pub purpose: Option<String>,       // 目的说明
  pub nbf: i64, pub exp: i64,
  pub attestation: String,           // 绑定签名（防伪）
}
```

### 2.5 消息与签名（`msg.rs`）

```rust
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum MsgType { Offer, Request, Notice, Receipt, Ack, Nack }

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MsgHeader {
  pub channel: ChannelId,
  pub from: PeerId, pub to: PeerId,
  pub kid: Kid,                          // 使用哪个公钥验证
  pub ts_ms: i64, pub seq: Seq, pub nonce: Nonce,
  pub policy_hash: String,               // 发送侧策略/能力快照
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Commitment { pub algo:&'static str, pub b64:String, pub size:u64 } // args/result 摘要

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MsgBody {
  pub msg_type: MsgType,
  pub capability: Option<CapabilityToken>,
  pub consent: Option<ConsentProof>,
  pub qos_estimate: Option<crate::model::attn::UsageSummary>, // 与 SB-14 对齐
  pub args_commit: Option<Commitment>,                         // 最小披露（承诺）
  pub payload_selective: serde_json::Value,                    // 选择性字段（必要最小）
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct A2AMessage {
  pub header: MsgHeader,
  pub body: MsgBody,                    // 明文（仅选择性字段）
  pub signature: String,                // 对 header+body（规范化序列化）的 JWS/COSE detached sig
}
```

### 2.6 回执与收据链（`receipt.rs`）

```rust
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct UsageSummary { pub tokens_in:u32, pub tokens_out:u32, pub bytes_in:u64, pub bytes_out:u64, pub cpu_ms:u64, pub amount_usd:Option<f32> }

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ReceiptBody {
  pub request_hash: Commitment,
  pub result_hash: Commitment,
  pub code: Option<String>,            // 稳定错误码
  pub usage: Option<UsageSummary>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Receipt {
  pub header: MsgHeader,               // 与请求同 channel/from/to/seq
  pub body: ReceiptBody,
  pub signature: String,               // 收据签名（对端）
}
```

------

## 3. 签名/验签与密钥轮换（`spi/signer.rs`）

```rust
#[async_trait::async_trait]
pub trait Signer: Send + Sync {
  fn kid(&self) -> Kid;
  async fn sign(&self, canonical: &[u8]) -> Result<String, A2aError>;     // 返回 JWS/COSE 字符串
}

#[async_trait::async_trait]
pub trait Verifier: Send + Sync {
  async fn verify(&self, kid:&Kid, canonical:&[u8], sig:&str) -> Result<(), A2aError>;
}

#[async_trait::async_trait]
pub trait KeyStore: Send + Sync {
  async fn current(&self) -> KeyMaterial;                                   // 本域当前私钥（给 Signer）
  async fn peer_keys(&self, peer:&PeerId) -> Vec<KeyMaterial>;              // 对端公钥集
  async fn rotate_local(&self, next:KeyMaterial) -> Result<(), A2aError>;   // 轮换
  async fn revoke(&self, kid:&Kid) -> Result<(), A2aError>;                 // 吊销
}
```

*规范化序列化*：对 `A2AMessage.header + body` 做稳定排序 JSON（或 CBOR）作为 `canonical`，以避免签名歧义。

------

## 4. 通道管理与反重放（`spi/channel.rs`, `spi/replay.rs`）

```rust
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum ChannelState { Opening, Active, Rotating, Closing, Closed }

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Channel {
  pub id: ChannelId, pub self_peer: PeerId, pub peer: PeerId,
  pub state: ChannelState,
  pub seq_out: Seq, pub seq_in_last: Seq,
  pub window_ms: i64,                        // 允许时钟偏差窗口
  pub policy_hash: String,                   // 协商后的交集策略哈希
  pub created_at: i64, pub updated_at: i64,
}

#[async_trait::async_trait]
pub trait ChannelStore: Send + Sync {
  async fn create(&self, c:Channel) -> Result<(), A2aError>;
  async fn get(&self, id:&ChannelId) -> Result<Channel, A2aError>;
  async fn update(&self, c:&Channel) -> Result<(), A2aError>;
}

#[async_trait::async_trait]
pub trait ChannelManager: Send + Sync {
  async fn open(&self, self_peer:PeerMetadata, peer:PeerMetadata) -> Result<ChannelId, A2aError>;
  async fn rotate(&self, id:&ChannelId, next_local:KeyMaterial) -> Result<(), A2aError>;
  async fn close(&self, id:&ChannelId) -> Result<(), A2aError>;
}

#[async_trait::async_trait]
pub trait ReplayGuard: Send + Sync {
  async fn check_inbound(&self, ch:&ChannelId, seq:Seq, ts_ms:i64, nonce:&Nonce) -> Result<(), A2aError>;
  async fn next_outbound(&self, ch:&ChannelId) -> Result<Seq, A2aError>;
}
```

*反重放实现原则*

- `check_inbound`：拒绝 `seq <= last_seq_in`；维护 `{nonce -> ts}` LRU；检查 `|ts_now - ts_ms| <= window_ms`；
- `next_outbound`：读增量序列，保证单调。

------

## 5. 传输与入站管线（`spi/transport.rs`, `spi/handler.rs`）

```rust
#[async_trait::async_trait]
pub trait Transport: Send + Sync {
  async fn send(&self, peer:&PeerMetadata, msg:&A2AMessage) -> Result<(), A2aError>;
  async fn send_receipt(&self, peer:&PeerMetadata, r:&Receipt) -> Result<(), A2aError>;
}

#[async_trait::async_trait]
pub trait InboundHandler: Send + Sync {
  async fn handle(&self, raw:&[u8]) -> Result<(), A2aError>; // 解码→验签→反重放→授权→路由
}
```

*入站流水线*

1. 解析 `A2AMessage`；
2. `Verifier.verify`（按 `kid`/指纹）→ 失败 `A2A.SIGNATURE_INVALID`；
3. `ReplayGuard.check_inbound` → 失败 `A2A.REPLAY`；
4. 能力/同意检查（对 `CapabilityToken` 与 `ConsentProof` 结构性验证 + 过期 + 绑定）；
5. 转交业务路由（例如工具执行、账页通知），产出 `Receipt`；
6. 记录 Evidence（SB-11）+ 入账（SB-14，若是结算通知）。

------

## 6. 门面（`facade.rs`）

```rust
pub struct A2AFacade<
  S:Signer, V:Verifier, KS:KeyStore, CM:ChannelManager, CS:ChannelStore, RG:ReplayGuard, T:Transport, RS:ReceiptStore
> { pub signer:S, pub verifier:V, pub keystore:KS, pub channels:CM, pub ch_store:CS, pub replay:RG, pub transport:T, pub receipts:RS }

impl<S,V,KS,CM,CS,RG,T,RS> A2AFacade<S,V,KS,CM,CS,RG,T,RS>
where S:Signer, V:Verifier, KS:KeyStore, CM:ChannelManager, CS:ChannelStore, RG:ReplayGuard, T:Transport, RS:ReceiptStore
{
  pub async fn offer(&self, ch:&ChannelId, offer:serde_json::Value) -> Result<(), A2aError> {
    let mut c = self.ch_store.get(ch).await?;
    let seq = self.replay.next_outbound(ch).await?;
    let msg = make_signed(self, &c, MsgType::Offer, seq, offer).await?;
    self.transport.send(&peer_meta(&c)?, &msg).await
  }

  pub async fn request(&self, ch:&ChannelId, body:MsgBody) -> Result<Receipt, A2aError> {
    let c = self.ch_store.get(ch).await?; let seq = self.replay.next_outbound(ch).await?;
    let msg = sign_full(self, &c, seq, body).await?;
    self.transport.send(&peer_meta(&c)?, &msg).await?;
    // 可选：等待对端回执（HTTP 同步或异步回来的 Receipt）
    // RIS：由上层保留等待逻辑；此处只负责发送
    Ok(empty_receipt_placeholder())
  }
}
```

> `make_signed/sign_full/peer_meta` 为内部辅助：构造 header、规范化、签名；`ReceiptStore` 负责存储与收据链合并。

------

## 7. 收据存储与证据（`spi/receipt_store.rs`, `observe.rs`）

```rust
#[async_trait::async_trait]
pub trait ReceiptStore: Send + Sync {
  async fn save_local(&self, r:&Receipt) -> Result<(), A2aError>;
  async fn save_remote(&self, r:&Receipt) -> Result<(), A2aError>;
  async fn chain_status(&self, req_hash:&Commitment) -> Result<(bool,bool), A2aError>; // (local_signed, remote_signed)
}
```

**证据事件（SB-11）**

- `A2AChannelOpen`, `A2AMsgSent`, `A2AMsgReceived`, `A2AVerifyFailed`, `A2AReplayBlocked`, `A2AReceiptLocal`, `A2AReceiptRemote`, `A2ALedgerDiff`
- 标签最小集：`tenant, peer, type, code`

------

## 8. 与 Tx / QoS / Observe 的对接

### 8.1 可靠投递（SB-10）

- 发送侧：`A2AMessage` 序列化 → **Outbox**（同业务写入同事务）→ Dispatcher 投递；
- 接收侧：处理完成后生成 `Receipt` → Outbox 回执 → 双签完成事件；
- 死信：进入 `dead_letters(kind="a2a")`，由运维回放。

### 8.2 记账与对账（SB-14）

- `ReceiptBody.usage.amount_usd` 可为空；结算时由双方各自 `settle()` 计算账页；
- 周期交换 `LedgerSummary`（同样以 A2A 消息封装）→ `Reconciler` 输出差异；
- 差异上报 `A2A.LEDGER_MISMATCH` 与报警。

### 8.3 观测（SB-11）

- 每步调用 `EvidenceSink.emit()` 与 `Meter` 打点；
- 所有错误走 `soulbase-errors` 稳定码，并写公共视图。

------

## 9. 存储结构（SurrealQL 摘要，`surreal/schema.surql`）

```sql
DEFINE TABLE a2a_channel SCHEMAFULL;
DEFINE FIELD id            ON a2a_channel TYPE string;       -- ChannelId
DEFINE FIELD self_peer     ON a2a_channel TYPE string;
DEFINE FIELD peer          ON a2a_channel TYPE string;
DEFINE FIELD state         ON a2a_channel TYPE string;
DEFINE FIELD seq_out       ON a2a_channel TYPE int;
DEFINE FIELD seq_in_last   ON a2a_channel TYPE int;
DEFINE FIELD window_ms     ON a2a_channel TYPE int;
DEFINE FIELD policy_hash   ON a2a_channel TYPE string;
DEFINE FIELD created_at    ON a2a_channel TYPE datetime;
DEFINE FIELD updated_at    ON a2a_channel TYPE datetime;
DEFINE INDEX pk_a2a_ch ON TABLE a2a_channel COLUMNS id UNIQUE;

DEFINE TABLE a2a_nonce SCHEMAFULL; -- 反重放窗内 Nonce 集
DEFINE FIELD ch      ON a2a_nonce TYPE string;
DEFINE FIELD nonce   ON a2a_nonce TYPE string;
DEFINE FIELD ts_ms   ON a2a_nonce TYPE datetime;
DEFINE INDEX idx_nonce ON TABLE a2a_nonce COLUMNS ch, nonce UNIQUE;

DEFINE TABLE a2a_msg SCHEMAFULL;
DEFINE FIELD ch      ON a2a_msg TYPE string;
DEFINE FIELD seq     ON a2a_msg TYPE int;
DEFINE FIELD dir     ON a2a_msg TYPE string;     -- out|in
DEFINE FIELD hdr     ON a2a_msg TYPE object;
DEFINE FIELD body    ON a2a_msg TYPE object;
DEFINE FIELD sig     ON a2a_msg TYPE string;
DEFINE FIELD code    ON a2a_msg TYPE string;     -- 验签/检查结果（如有）
DEFINE FIELD created_at ON a2a_msg TYPE datetime;
DEFINE INDEX idx_msg ON TABLE a2a_msg COLUMNS ch, seq, dir;

DEFINE TABLE a2a_receipt SCHEMAFULL;
DEFINE FIELD ch        ON a2a_receipt TYPE string;
DEFINE FIELD seq       ON a2a_receipt TYPE int;
DEFINE FIELD local     ON a2a_receipt TYPE object;  -- 本地签名收据（可空）
DEFINE FIELD remote    ON a2a_receipt TYPE object;  -- 对端签名收据（可空）
DEFINE FIELD status    ON a2a_receipt TYPE string;  -- open|half|full
DEFINE INDEX idx_rcpt  ON TABLE a2a_receipt COLUMNS ch, seq UNIQUE;
```

------

## 10. 错误映射（`errors.rs`）

```rust
use thiserror::Error;
use soulbase_errors::prelude::*;
#[derive(Debug, Error)]
#[error("{0}")]
pub struct A2aError(pub ErrorObj);

impl A2aError {
  pub fn signature_invalid(msg:&str)->Self { A2aError(ErrorBuilder::new(codes::UNKNOWN_INTERNAL /* A2A.SIGNATURE_INVALID */).user_msg("Invalid signature.").dev_msg(msg).build()) }
  pub fn replay()->Self { A2aError(ErrorBuilder::new(codes::UNKNOWN_INTERNAL /* A2A.REPLAY */).user_msg("Replay detected.").build()) }
  pub fn consent_required()->Self { A2aError(ErrorBuilder::new(codes::UNKNOWN_INTERNAL /* A2A.CONSENT_REQUIRED */).user_msg("Consent required.").build()) }
  pub fn capability_deny()->Self { A2aError(ErrorBuilder::new(codes::UNKNOWN_INTERNAL /* A2A.CAPABILITY_DENY */).user_msg("Capability denied.").build()) }
  pub fn provider_unavailable(msg:&str)->Self { A2aError(ErrorBuilder::new(codes::PROVIDER_UNAVAILABLE).user_msg("Peer unavailable.").dev_msg(msg).build()) }
  pub fn schema(msg:&str)->Self { A2aError(ErrorBuilder::new(codes::SCHEMA_VAILDATION /* fix code name */).user_msg("Invalid A2A payload.").dev_msg(msg).build()) }
}
```

> 说明：在 `soulbase-errors` 中建议新增 `A2A.*` 码（TD 用占位，RIS 时与码表对齐）。

------

## 11. 热更与轮换策略

- **密钥轮换**：`ChannelState::Rotating` 期间 **并行验证** 新旧 kid；`not_before/not_after` 控制切换窗；
- **策略/价目/留存**：`policy_hash/pricing_version/retention_version` 固化在消息头，作为证据与结算用的**快照指纹**；
- **撤销清单**：通过 `Notice{revocations}` 定期推送；接收端更新内存表+持久化，并在 `InboundPipeline` 强校验。

------

## 12. 测试与验收（建议用例）

- **签名/验签**：正常通过/篡改失败；
- **反重放**：重复 `seq/nonce`、过窗 `ts`；
- **能力/同意**：缺少或过期→拒绝；
- **双签收据**：请求→回执→本地签收→链状态 `full`；
- **与 Tx**：Outbox 死信后回放重试成功；
- **与 QoS**：预留→实际结算→账页一致；
- **与 Observe**：指标与 Evidence 覆盖率 100%。

------

## 13. 安全附注

- **最小披露**：默认只发 `args_commit`；若确需明文字段，必须在 `capability/consent` 中明确；
- **抗重放**：`seq + ts + nonce` + $可选$ `channel-bound HMAC`；
- **抗抵赖**：双签收据链 + 本地证据不可变存储（WORM/归档）；
- **时钟**：允许 ±300s 偏差；更大偏差 → `A2A.CLOCK_SKEW`（可选码）。

------

### 小结

本 TD 给出了 `soulbase-a2a` 的**结构化消息/收据/凭证模型、签名与反重放 SPI、通道状态机与持久化、入站验证与业务路由、与 Tx/QoS/Observe 的衔接**。
 若方向一致，下一步我将按“三件套”输出 **SB-15-RIS（最小可运行骨架）**：提供 **JWS/COSE 伪实现（内存密钥）**、**InMemoryChannelStore/ReplayGuard/ReceiptStore**、**HTTP 传输桩**、**A2AFacade 示例** 与 **2–3 个端到端单测**（签名/反重放/双签收据）。
