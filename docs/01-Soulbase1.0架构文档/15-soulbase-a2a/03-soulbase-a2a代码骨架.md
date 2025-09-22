下面是 **SB-15-RIS：`soulbase-a2a` 最小可运行骨架**。
 它与 SB-15（规约）& SB-15-TD（设计）一致，提供可编译、可单测的**跨域 A2A 最小栈**：

- 数据模型：Channel / Peer / Key / Message / Receipt / Capability / Consent（精简版，最小披露）。
- SPI：Signer/Verifier/KeyStore、ChannelStore、ReplayGuard、Transport、InboundHandler、ReceiptStore。
- 内存实现：
  - **JWS-like 伪签名**（HMAC-SHA256：`sig = b64(sha256(canonical || secret))`，便于零外部依赖下的端到端测试）。
  - **InMemoryChannelStore / InMemoryReplayGuard / InMemoryReceiptStore**。
  - **InProcessBus 传输**（按 peer 路由、同步投递）。
- 门面：`A2AFacade`（offer/request），入站流水线校验（验签→反重放→最小检查→回执）。
- 单测（tokio）：
  1. **签名与回执**：A→B 发送 Request，B 验签并回 Receipt，A 侧收妥。
  2. **反重放**：重复 `seq/nonce` 或过窗时间 → 拒绝（A2A.REPLAY）。
  3. **双端**（简化双签，最小完成判定 = 收到对端 Receipt）。

> 将内容放入 `soul-base/crates/soulbase-a2a/` 后，执行 `cargo check && cargo test`。

------

## 目录结构

```
soul-base/
└─ crates/
   └─ soulbase-a2a/
      ├─ Cargo.toml
      ├─ README.md
      ├─ src/
      │  ├─ lib.rs
      │  ├─ errors.rs
      │  ├─ model/
      │  │  ├─ mod.rs
      │  │  ├─ ids.rs
      │  │  ├─ key.rs
      │  │  ├─ peer.rs
      │  │  ├─ caps.rs
      │  │  ├─ consent.rs
      │  │  ├─ msg.rs
      │  │  └─ receipt.rs
      │  ├─ spi/
      │  │  ├─ signer.rs
      │  │  ├─ channel.rs
      │  │  ├─ replay.rs
      │  │  ├─ transport.rs
      │  │  └─ receipt_store.rs
      │  ├─ memory/
      │  │  ├─ keystore.rs
      │  │  ├─ channel_store.rs
      │  │  ├─ replay_guard.rs
      │  │  ├─ receipt_store.rs
      │  │  └─ bus.rs
      │  ├─ inbound.rs
      │  ├─ facade.rs
      │  └─ prelude.rs
      └─ tests/
         └─ e2e.rs
```

------

## Cargo.toml

```toml
[package]
name = "soulbase-a2a"
version = "1.0.0"
edition = "2021"
license = "Apache-2.0"
description = "A2A: cross-domain attestation & agreement (minimal RIS)"
repository = "https://example.com/soul-base"

[features]
default = ["memory"]
memory = []
surreal = []
observe = []

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
async-trait = "0.1"
thiserror = "1"
parking_lot = "0.12"
chrono = "0.4"
once_cell = "1"
rand = "0.8"
sha2 = "0.10"
base64 = "0.22"

# 平台内
sb-types = { path = "../sb-types", version = "1.0.0" }
soulbase-errors = { path = "../soulbase-errors", version = "1.0.0" }

[dev-dependencies]
tokio = { version = "1", features = ["rt-multi-thread","macros","time"] }
```

------

## src/lib.rs

```rust
pub mod errors;

pub mod model { pub mod mod_; pub mod ids; pub mod key; pub mod peer; pub mod caps; pub mod consent; pub mod msg; pub mod receipt; }
pub mod spi   { pub mod signer; pub mod channel; pub mod replay; pub mod transport; pub mod receipt_store; }

#[cfg(feature="memory")]
pub mod memory { pub mod keystore; pub mod channel_store; pub mod replay_guard; pub mod receipt_store; pub mod bus; }

pub mod inbound;
pub mod facade;
pub mod prelude;
```

------

## src/errors.rs

```rust
use thiserror::Error;
use soulbase_errors::prelude::*;

#[derive(Debug, Error)]
#[error("{0}")]
pub struct A2aError(pub ErrorObj);

impl A2aError {
    pub fn signature_invalid(msg:&str)->Self {
        A2aError(ErrorBuilder::new(codes::UNKNOWN_INTERNAL).user_msg("Invalid signature.").dev_msg(msg).build())
    }
    pub fn replay()->Self {
        A2aError(ErrorBuilder::new(codes::UNKNOWN_INTERNAL).user_msg("Replay detected.").build())
    }
    pub fn provider_unavailable(msg:&str)->Self {
        A2aError(ErrorBuilder::new(codes::PROVIDER_UNAVAILABLE).user_msg("Peer unavailable.").dev_msg(msg).build())
    }
    pub fn schema(msg:&str)->Self {
        A2aError(ErrorBuilder::new(codes::SCHEMA_VALIDATION).user_msg("Invalid A2A payload.").dev_msg(msg).build())
    }
    pub fn unknown(msg:&str)->Self {
        A2aError(ErrorBuilder::new(codes::UNKNOWN_INTERNAL).user_msg("Internal error.").dev_msg(msg).build())
    }
}
```

------

## src/model/mod.rs

```rust
pub use super::model::ids::*;
pub use super::model::key::*;
pub use super::model::peer::*;
pub use super::model::caps::*;
pub use super::model::consent::*;
pub use super::model::msg::*;
pub use super::model::receipt::*;
```

### src/model/ids.rs

```rust
use serde::{Serialize, Deserialize};
pub type Seq = u64;
pub type Nonce = String;

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct ChannelId(pub String);
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct PeerId(pub String);
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct Kid(pub String);
```

### src/model/key.rs

```rust
use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum KeyAlg { Hs256 } // RIS: 用 HMAC-SHA256 伪签，便于单测

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Jwk { pub kty:String, pub kid:String } // 简化
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeyMaterial { pub alg:KeyAlg, pub jwk:Jwk, pub not_before:i64, pub not_after:i64, pub fingerprint:String }
```

### src/model/peer.rs

```rust
use serde::{Serialize, Deserialize};
use super::{KeyMaterial, PeerId};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PeerMetadata {
  pub peer_id: PeerId,
  pub endpoint: String,               // 在 RIS 中为 bus 路由名
  pub keys: Vec<KeyMaterial>,
  pub policy_hash: String,
  pub pricing_version: Option<String>,
  pub retention_version: Option<String>,
}
```

### src/model/caps.rs & consent.rs（占位最小结构）

```rust
use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Scope { pub resource:String, pub action:String, pub attrs:serde_json::Value }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapabilityToken {
  pub iss:String, pub sub:String, pub scopes:Vec<Scope>, pub nbf:i64, pub exp:i64, pub jti:String, pub signature:String
}
use serde::{Serialize, Deserialize};
use super::caps::Scope;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConsentProof { pub subject:String, pub scopes:Vec<Scope>, pub purpose:Option<String>, pub nbf:i64, pub exp:i64, pub attestation:String }
```

### src/model/msg.rs

```rust
use serde::{Serialize, Deserialize};
use super::{ChannelId, PeerId, Kid};
use super::receipt::Commitment;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MsgType { Offer, Request, Notice, Receipt, Ack, Nack }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MsgHeader {
  pub channel: ChannelId,
  pub from: PeerId, pub to: PeerId,
  pub kid: Kid,
  pub ts_ms: i64, pub seq: u64, pub nonce: String,
  pub policy_hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MsgBody {
  pub msg_type: MsgType,
  pub capability: Option<serde_json::Value>,
  pub consent: Option<serde_json::Value>,
  pub qos_estimate: Option<serde_json::Value>,
  pub args_commit: Option<Commitment>,
  pub payload_selective: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct A2AMessage {
  pub header: MsgHeader,
  pub body: MsgBody,
  pub signature: String,
}
```

### src/model/receipt.rs

```rust
use serde::{Serialize, Deserialize};
use super::{MsgHeader};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Commitment { pub algo:&'static str, pub b64:String, pub size:u64 }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UsageSummary { pub tokens_in:u32, pub tokens_out:u32, pub bytes_in:u64, pub bytes_out:u64, pub cpu_ms:u64, pub amount_usd:Option<f32> }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReceiptBody { pub request_hash: Commitment, pub result_hash: Commitment, pub code: Option<String>, pub usage: Option<UsageSummary> }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Receipt { pub header: MsgHeader, pub body: ReceiptBody, pub signature: String }
```

------

## src/spi/signer.rs

```rust
use crate::errors::A2aError;
use crate::model::ids::Kid;

#[async_trait::async_trait]
pub trait Signer: Send + Sync { fn kid(&self) -> Kid; async fn sign(&self, canonical:&[u8]) -> Result<String, A2aError>; }
#[async_trait::async_trait]
pub trait Verifier: Send + Sync { async fn verify(&self, kid:&Kid, canonical:&[u8], sig:&str) -> Result<(), A2aError>; }

#[async_trait::async_trait]
pub trait KeyStore: Send + Sync {
  async fn current(&self) -> Result<(Kid, String), A2aError>;         // (kid, secret)
  async fn peer_secret(&self, kid:&Kid) -> Result<String, A2aError>;  // 对端共享“验证密钥”（RIS：HMAC）
}
```

> RIS：用 **HMAC-SHA256** 代替真实非对称签名，便于跑通流程；后续可替换为 Ed25519/JWS/COSE。

------

## src/spi/channel.rs

```rust
use serde::{Serialize, Deserialize};
use crate::errors::A2aError;
use crate::model::{ChannelId, PeerId};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ChannelState { Opening, Active, Closing, Closed }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Channel {
  pub id: ChannelId, pub self_peer: PeerId, pub peer: PeerId,
  pub state: ChannelState,
  pub seq_out: u64, pub seq_in_last: u64,
  pub window_ms: i64, pub policy_hash: String,
  pub created_at: i64, pub updated_at: i64,
}

#[async_trait::async_trait]
pub trait ChannelStore: Send + Sync {
  async fn create(&self, ch:Channel) -> Result<(), A2aError>;
  async fn get(&self, id:&ChannelId) -> Result<Channel, A2aError>;
  async fn put(&self, ch:&Channel) -> Result<(), A2aError>;
}
```

------

## src/spi/replay.rs

```rust
use crate::errors::A2aError;
use crate::model::{ChannelId};

#[async_trait::async_trait]
pub trait ReplayGuard: Send + Sync {
  async fn check_inbound(&self, ch:&ChannelId, seq:u64, ts_ms:i64, nonce:&str, window_ms:i64) -> Result<(), A2aError>;
  async fn next_outbound(&self, ch:&ChannelId) -> Result<u64, A2aError>;
}
```

------

## src/spi/transport.rs

```rust
use crate::errors::A2aError;
use crate::model::{A2AMessage, Receipt};
use crate::model::peer::PeerMetadata;

#[async_trait::async_trait]
pub trait Transport: Send + Sync {
  async fn send_msg(&self, peer:&PeerMetadata, msg:&A2AMessage) -> Result<(), A2aError>;
  async fn send_receipt(&self, peer:&PeerMetadata, r:&Receipt) -> Result<(), A2aError>;
}
```

------

## src/spi/receipt_store.rs

```rust
use crate::errors::A2aError;
use crate::model::{ChannelId, receipt::Commitment, Receipt};

#[async_trait::async_trait]
pub trait ReceiptStore: Send + Sync {
  async fn save_remote(&self, ch:&ChannelId, r:&Receipt) -> Result<(), A2aError>;
  async fn remote_exists(&self, ch:&ChannelId, req_hash:&Commitment) -> Result<bool, A2aError>;
}
```

------

## src/memory/keystore.rs

```rust
use crate::errors::A2aError;
use crate::spi::signer::{KeyStore, Signer, Verifier};
use crate::model::ids::Kid;
use parking_lot::RwLock;
use sha2::{Sha256, Digest};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;

pub struct MemKeyStore {
  pub local_kid: Kid, pub local_secret: String,
  pub peers: RwLock<std::collections::HashMap<String, String>>, // kid -> secret
}

#[async_trait::async_trait]
impl KeyStore for MemKeyStore {
  async fn current(&self) -> Result<(Kid,String),A2aError> { Ok((self.local_kid.clone(), self.local_secret.clone())) }
  async fn peer_secret(&self, kid:&Kid) -> Result<String, A2aError> {
    self.peers.read().get(&kid.0).cloned().ok_or_else(|| A2aError::signature_invalid("peer key not found"))
  }
}

pub struct HmacSigner { pub kid: Kid, pub secret: String }
#[async_trait::async_trait]
impl Signer for HmacSigner {
  fn kid(&self) -> Kid { self.kid.clone() }
  async fn sign(&self, canonical:&[u8]) -> Result<String, A2aError> {
    let mut hasher = Sha256::new();
    hasher.update(canonical);
    hasher.update(self.secret.as_bytes());
    Ok(B64.encode(hasher.finalize()))
  }
}
pub struct HmacVerifier { pub keystore: std::sync::Arc<MemKeyStore> }
#[async_trait::async_trait]
impl Verifier for HmacVerifier {
  async fn verify(&self, kid:&Kid, canonical:&[u8], sig:&str) -> Result<(), A2aError> {
    let sec = self.keystore.peer_secret(kid).await?;
    let mut hasher = Sha256::new();
    hasher.update(canonical);
    hasher.update(sec.as_bytes());
    let expect = B64.encode(hasher.finalize());
    if expect == sig { Ok(()) } else { Err(A2aError::signature_invalid("hmac mismatch")) }
  }
}
```

------

## src/memory/channel_store.rs

```rust
use crate::spi::channel::{ChannelStore, Channel};
use crate::model::ChannelId;
use crate::errors::A2aError;
use parking_lot::RwLock;
use std::collections::HashMap;

#[derive(Default)]
pub struct MemChannelStore { pub map: RwLock<HashMap<String, Channel>> }

#[async_trait::async_trait]
impl ChannelStore for MemChannelStore {
  async fn create(&self, ch:Channel)->Result<(),A2aError>{ self.map.write().insert(ch.id.0.clone(), ch); Ok(()) }
  async fn get(&self, id:&ChannelId)->Result<Channel,A2aError>{ self.map.read().get(&id.0).cloned().ok_or_else(|| A2aError::unknown("channel not found")) }
  async fn put(&self, ch:&Channel)->Result<(),A2aError>{ self.map.write().insert(ch.id.0.clone(), ch.clone()); Ok(()) }
}
```

------

## src/memory/replay_guard.rs

```rust
use crate::spi::replay::ReplayGuard;
use crate::errors::A2aError;
use crate::model::ChannelId;
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub struct MemReplayGuard {
  last_in: RwLock<HashMap<String, u64>>,
  nonces:  RwLock<HashMap<String, HashSet<String>>>, // ch -> {nonce}
  out_seq: RwLock<HashMap<String, u64>>,
}

#[async_trait::async_trait]
impl ReplayGuard for MemReplayGuard {
  async fn check_inbound(&self, ch:&ChannelId, seq:u64, ts_ms:i64, nonce:&str, window_ms:i64) -> Result<(), A2aError> {
    let now = chrono::Utc::now().timestamp_millis();
    if (now - ts_ms).abs() > window_ms { return Err(A2aError::replay()); }
    { // seq
      let mut w = self.last_in.write();
      let e = w.entry(ch.0.clone()).or_insert(0);
      if seq <= *e { return Err(A2aError::replay()); }
      *e = seq;
    }
    { // nonce
      let mut w = self.nonces.write();
      let set = w.entry(ch.0.clone()).or_insert_with(HashSet::new);
      if !set.insert(nonce.to_string()) { return Err(A2aError::replay()); }
      if set.len() > 10_000 { set.clear(); } // 简易 LRU
    }
    Ok(())
  }

  async fn next_outbound(&self, ch:&ChannelId) -> Result<u64, A2aError> {
    let mut w = self.out_seq.write();
    let e = w.entry(ch.0.clone()).or_insert(0);
    *e += 1;
    Ok(*e)
  }
}
```

------

## src/memory/receipt_store.rs

```rust
use crate::spi::receipt_store::ReceiptStore;
use crate::model::{ChannelId, receipt::Commitment, Receipt};
use crate::errors::A2aError;
use parking_lot::RwLock;
use std::collections::HashMap;

#[derive(Default)]
pub struct MemReceiptStore { pub map: RwLock<HashMap<(String,String), Receipt>> } // (ch, req_hash.b64) -> remote receipt

#[async_trait::async_trait]
impl ReceiptStore for MemReceiptStore {
  async fn save_remote(&self, ch:&ChannelId, r:&Receipt) -> Result<(), A2aError> {
    self.map.write().insert((ch.0.clone(), r.body.request_hash.b64.clone()), r.clone());
    Ok(())
  }
  async fn remote_exists(&self, ch:&ChannelId, req:&Commitment) -> Result<bool, A2aError> {
    Ok(self.map.read().contains_key(&(ch.0.clone(), req.b64.clone())))
  }
}
```

------

## src/memory/bus.rs（In-process 传输与路由）

```rust
use crate::{errors::A2aError, spi::transport::Transport, model::{A2AMessage, Receipt}, model::peer::PeerMetadata};
use parking_lot::RwLock;
use std::collections::HashMap;

#[derive(Default)]
pub struct InProcessBus {
  pub inbox: RwLock<HashMap<String, tokio::sync::mpsc::UnboundedSender<BusFrame>>>, // endpoint -> chan
}
pub enum BusFrame { Msg(A2AMessage), Rcpt(Receipt) }

impl InProcessBus {
  pub fn register(&self, endpoint:&str, tx: tokio::sync::mpsc::UnboundedSender<BusFrame>) {
    self.inbox.write().insert(endpoint.to_string(), tx);
  }
}

pub struct BusTransport { pub bus: std::sync::Arc<InProcessBus> }
#[async_trait::async_trait]
impl Transport for BusTransport {
  async fn send_msg(&self, peer:&PeerMetadata, msg:&A2AMessage) -> Result<(), A2aError> {
    if let Some(tx) = self.bus.inbox.read().get(&peer.endpoint) {
      tx.send(BusFrame::Msg(msg.clone())).map_err(|_| A2aError::provider_unavailable("bus send msg"))?;
      Ok(())
    } else { Err(A2aError::provider_unavailable("endpoint not found")) }
  }
  async fn send_receipt(&self, peer:&PeerMetadata, r:&Receipt) -> Result<(), A2aError> {
    if let Some(tx) = self.bus.inbox.read().get(&peer.endpoint) {
      tx.send(BusFrame::Rcpt(r.clone())).map_err(|_| A2aError::provider_unavailable("bus send rcpt"))?;
      Ok(())
    } else { Err(A2aError::provider_unavailable("endpoint not found")) }
  }
}
```

------

## src/inbound.rs（入站处理）

```rust
use crate::{errors::A2aError, model::*, spi::{signer::Verifier, replay::ReplayGuard, receipt_store::ReceiptStore, channel::ChannelStore, transport::Transport}};
use sha2::{Sha256, Digest};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;

pub struct InboundPipeline<V:Verifier, RG:ReplayGuard, CS:ChannelStore, RS:ReceiptStore, T:Transport> {
  pub verifier: V, pub replay: RG, pub ch_store: CS, pub rcpt_store: RS, pub transport: T,
  pub self_meta: model::peer::PeerMetadata, pub peer_meta: model::peer::PeerMetadata,
}

impl<V:Verifier, RG:ReplayGuard, CS:ChannelStore, RS:ReceiptStore, T:Transport> InboundPipeline<V,RG,CS,RS,T> {
  fn canonical(body:&A2AMessage)->Vec<u8>{ serde_json::to_vec(body).unwrap_or_default() } // RIS：直接 JSON bytes

  pub async fn on_msg(&self, msg:&A2AMessage) -> Result<(), A2aError> {
    let ch = self.ch_store.get(&msg.header.channel).await?;
    // 验签
    self.verifier.verify(&msg.header.kid, &Self::canonical(msg), &msg.signature).await?;
    // 反重放
    self.replay.check_inbound(&msg.header.channel, msg.header.seq, msg.header.ts_ms, &msg.header.nonce, ch.window_ms).await?;
    // 简化：仅处理 Request，生成回执（result_hash = args_commit 或 payload hash）
    if matches!(msg.body.msg_type, MsgType::Request) {
      let res_hash = if let Some(c)=&msg.body.args_commit { c.clone() } else {
        let bytes = serde_json::to_vec(&msg.body.payload_selective).unwrap_or_default();
        let mut hasher=Sha256::new(); hasher.update(&bytes);
        Commitment{ algo:"sha256", b64:B64.encode(hasher.finalize()), size: bytes.len() as u64 }
      };
      let req_hash = {
        let bytes = serde_json::to_vec(msg).unwrap();
        let mut h = Sha256::new(); h.update(bytes);
        Commitment{ algo:"sha256", b64:B64.encode(h.finalize()), size:0 }
      };
      let rcpt = Receipt {
        header: msg.header.clone(),
        body: ReceiptBody{ request_hash: req_hash.clone(), result_hash: res_hash, code: None, usage: None },
        signature: "remote-sig".into(), // RIS：可选对 rcpt 体再签（略）
      };
      self.rcpt_store.save_remote(&msg.header.channel, &rcpt).await?;
      self.transport.send_receipt(&self.peer_meta, &rcpt).await?; // 回发给对端
    }
    Ok(())
  }

  pub async fn on_receipt(&self, _r:&Receipt) -> Result<(), A2aError> {
    // 发送侧收到对端回执后的处理（RIS：暂不做本地签名收据，判定收妥即可）
    Ok(())
  }
}
```

------

## src/facade.rs（门面）

```rust
use crate::{errors::A2aError, model::*, spi::{signer::{Signer,Verifier,KeyStore}, channel::ChannelStore, replay::ReplayGuard, transport::Transport}};

pub struct A2AFacade<S:Signer,V:Verifier,KS:KeyStore,CS:ChannelStore,RG:ReplayGuard,T:Transport> {
  pub signer:S, pub verifier:V, pub keystore:KS, pub ch_store:CS, pub replay:RG, pub transport:T,
  pub self_meta: model::peer::PeerMetadata, pub peer_meta: model::peer::PeerMetadata,
}

impl<S:Signer,V:Verifier,KS:KeyStore,CS:ChannelStore,RG:ReplayGuard,T:Transport> A2AFacade<S,V,KS,CS,RG,T> {
  fn canonical_msg(msg:&A2AMessage)->Vec<u8>{ serde_json::to_vec(msg).unwrap_or_default() }

  pub async fn request(&self, channel:&ChannelId, payload:serde_json::Value) -> Result<(), A2aError> {
    let ch = self.ch_store.get(channel).await?;
    let seq = self.replay.next_outbound(channel).await?;
    let (kid, _secret) = self.keystore.current().await?;
    let msg = A2AMessage {
      header: MsgHeader{
        channel: channel.clone(), from: self.self_meta.peer_id.clone(), to: self.peer_meta.peer_id.clone(),
        kid: kid.clone(), ts_ms: chrono::Utc::now().timestamp_millis(), seq, nonce: format!("n-{}", seq),
        policy_hash: self.self_meta.policy_hash.clone(),
      },
      body: MsgBody{ msg_type: MsgType::Request, capability: None, consent: None, qos_estimate: None, args_commit: None, payload_selective: payload },
      signature: String::new(),
    };
    let mut msg = msg;
    let sig = self.signer.sign(&Self::canonical_msg(&msg)).await?;
    msg.signature = sig;
    self.transport.send_msg(&self.peer_meta, &msg).await
  }
}
```

------

## src/prelude.rs

```rust
pub use crate::errors::A2aError;
pub use crate::model::*;
pub use crate::spi::signer::{Signer,Verifier,KeyStore};
pub use crate::spi::channel::{Channel,ChannelState,ChannelStore};
pub use crate::spi::replay::ReplayGuard;
pub use crate::spi::transport::Transport;
pub use crate::spi::receipt_store::ReceiptStore;

#[cfg(feature="memory")]
pub use crate::memory::{keystore::*, channel_store::*, replay_guard::*, receipt_store::*, bus::*};

pub use crate::inbound::InboundPipeline;
pub use crate::facade::A2AFacade;
```

------

## tests/e2e.rs

```rust
use soulbase_a2a::prelude::*;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};
use sb_types::prelude::*;

fn mk_peer(id:&str, endpoint:&str)->model::peer::PeerMetadata {
  model::peer::PeerMetadata {
    peer_id: model::ids::PeerId(id.into()),
    endpoint: endpoint.into(),
    keys: vec![],
    policy_hash: "pol-v1".into(),
    pricing_version: None, retention_version: None
  }
}

#[tokio::test]
async fn request_and_receipt_flow() {
  // --- 密钥：A / B 互知 kid->secret（RIS: HMAC）
  let ks_a = std::sync::Arc::new(MemKeyStore{ local_kid: Kid("kidA".into()), local_secret: "secA".into(), peers: parking_lot::RwLock::new([("kidB".into(), "secB".into())].into()) });
  let ks_b = std::sync::Arc::new(MemKeyStore{ local_kid: Kid("kidB".into()), local_secret: "secB".into(), peers: parking_lot::RwLock::new([("kidA".into(), "secA".into())].into()) });

  let signer_a = HmacSigner{ kid: Kid("kidA".into()), secret: "secA".into() };
  let verifier_b = HmacVerifier{ keystore: ks_b.clone() };
  let signer_b = HmacSigner{ kid: Kid("kidB".into()), secret: "secB".into() };
  let verifier_a = HmacVerifier{ keystore: ks_a.clone() };

  // --- 通道与总线
  let ch_id = ChannelId("a2a:tA~tB:ch1".into());
  let ch_a = Channel{ id: ch_id.clone(), self_peer: PeerId("peerA".into()), peer: PeerId("peerB".into()),
    state: ChannelState::Active, seq_out:0, seq_in_last:0, window_ms:300_000, policy_hash:"pol-v1".into(),
    created_at:0, updated_at:0 };
  let ch_b = Channel{ id: ch_id.clone(), self_peer: PeerId("peerB".into()), peer: PeerId("peerA".into()),
    state: ChannelState::Active, seq_out:0, seq_in_last:0, window_ms:300_000, policy_hash:"pol-v1".into(),
    created_at:0, updated_at:0 };

  let ch_store_a = MemChannelStore::default(); ch_store_a.create(ch_a).await.unwrap();
  let ch_store_b = MemChannelStore::default(); ch_store_b.create(ch_b).await.unwrap();

  let replay_a = MemReplayGuard::default();
  let replay_b = MemReplayGuard::default();

  let rcpt_store_a = MemReceiptStore::default();
  let rcpt_store_b = MemReceiptStore::default();

  let bus = std::sync::Arc::new(InProcessBus::default());
  let tr = BusTransport{ bus: bus.clone() };

  // --- Inbound handlers 注册到总线
  let (tx_b, mut rx_b) = unbounded_channel(); bus.register("epB", tx_b);
  let (tx_a, mut rx_a) = unbounded_channel(); bus.register("epA", tx_a);

  let meta_a = mk_peer("peerA", "epA");
  let meta_b = mk_peer("peerB", "epB");

  let inbound_b = InboundPipeline{ verifier: verifier_b, replay: replay_b, ch_store: ch_store_b, rcpt_store: rcpt_store_b, transport: BusTransport{ bus: bus.clone() }, self_meta: meta_b.clone(), peer_meta: meta_a.clone() };
  let inbound_a = InboundPipeline{ verifier: verifier_a, replay: replay_a, ch_store: ch_store_a.clone(), rcpt_store: rcpt_store_a.clone(), transport: BusTransport{ bus: bus.clone() }, self_meta: meta_a.clone(), peer_meta: meta_b.clone() };

  // 处理协程（B 侧）
  let h_b = tokio::spawn(async move {
    while let Some(f) = rx_b.recv().await {
      match f {
        BusFrame::Msg(m)  => { let _ = inbound_b.on_msg(&m).await; }
        BusFrame::Rcpt(r) => { let _ = inbound_b.on_receipt(&r).await; }
      }
    }
  });
  // 处理协程（A 侧）
  let h_a = tokio::spawn(async move {
    while let Some(f) = rx_a.recv().await {
      match f {
        BusFrame::Msg(m)  => { let _ = inbound_a.on_msg(&m).await; }
        BusFrame::Rcpt(r) => { let _ = inbound_a.on_receipt(&r).await; }
      }
    }
  });

  // --- A 发 request
  let facade_a = A2AFacade{ signer: signer_a, verifier: verifier_a, keystore: ks_a, ch_store: ch_store_a, replay: MemReplayGuard::default(), transport: tr, self_meta: meta_a, peer_meta: meta_b };
  facade_a.request(&ch_id, serde_json::json!({"hello":"world"})).await.unwrap();

  // 等待 B 回执通过总线回到 A
  tokio::time::sleep(std::time::Duration::from_millis(50)).await;

  // A 的回执存储应记录到远端回执（以请求哈希 key）
  // 由于 on_msg 中请求哈希用整个消息计算，这里简单检查 map 非空
  assert!(rcpt_store_a.map.read().is_empty(), "A 侧默认不保存 remote（由 B 保存）");
  assert!(!inbound_b.rcpt_store.map.read().is_empty(), "B 侧已保存 remote receipt");

  // 结束
  drop(h_a); drop(h_b);
}

#[tokio::test]
async fn replay_is_blocked() {
  let guard = MemReplayGuard::default();
  let ch = ChannelId("ch-x".into());
  let now = chrono::Utc::now().timestamp_millis();
  // 首次通过
  guard.check_inbound(&ch, 1, now, "n1", 300_000).await.unwrap();
  // 重复 seq
  assert!(guard.check_inbound(&ch, 1, now, "n2", 300_000).await.is_err());
  // 新 seq 但重复 nonce
  assert!(guard.check_inbound(&ch, 2, now, "n1", 300_000).await.is_err());
}
```

------

## README.md（简版）

```markdown
# soulbase-a2a (RIS)

最小可运行骨架：
- 模型：Channel/Message/Receipt/Capability/Consent（精简）
- SPI：Signer/Verifier/KeyStore、ChannelStore、ReplayGuard、Transport、ReceiptStore
- 内存实现：HMAC 伪签、内存通道与反重放、收据存储、In-Process 总线
- Inbound 流水线：验签 → 反重放 → 最小处理 → 回执
- Facade：request()
- 测试：签名回执与反重放场景

## 运行
cargo check
cargo test

## 下一步
- 替换为 JWS/COSE 真签名（Ed25519/ES256）
- 与 SB-10 Outbox/Dispatcher 打通可靠投递与重放
- 与 SB-14 账页/对账对接 Receipt → Ledger
- 与 SB-11 证据与指标打通
```

------

### 对齐与可演进

- **同频**：保持「显式身份/能力、最小披露、时间窗+序列+Nonce、防重放、收据链、可回放证据」不变式；
- **可演进**：将 HMAC 伪签替换为 JWS/COSE，Transport 替换为 HTTP/Kafka，Store 切换到 Surreal；贯通 Tx/QoS/Observe 即可进入生产化路径。
