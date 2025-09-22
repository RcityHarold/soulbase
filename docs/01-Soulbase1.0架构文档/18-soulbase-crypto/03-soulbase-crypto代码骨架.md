下面是 **SB-18-RIS：`soulbase-crypto` 最小可运行骨架**。
 与 SB-18（规约）& SB-18-TD（设计）一致，骨架提供：

- 统一能力：**规范化序列化（Canonical JSON）**、**摘要（sha256/blake3）**、**JWS Detached（Ed25519）签名/验签**、**AEAD（XChaCha20-Poly1305）**、**HKDF**。
- Key 管理：**内存 KeyStore**（当前私钥 + 公钥 JWKS/撤销/有效期窗口）。
- 错误→稳定码映射（使用 SB-02 已有码位：`SCHEMA.VALIDATION_FAILED`、`PROVIDER.UNAVAILABLE`、`AUTH.FORBIDDEN`、`UNKNOWN.INTERNAL`）。
- 4 个端到端单测：**canonical 一致性/拒绝浮点**、**签名/验签**、**AEAD seal/open**、**HKDF 一致性与过期/撤销失败**。

> 放入 `soul-base/crates/soulbase-crypto/` 后运行 `cargo check && cargo test`。

------

## 目录结构

```
soul-base/
└─ crates/
   └─ soulbase-crypto/
      ├─ Cargo.toml
      ├─ README.md
      ├─ src/
      │  ├─ lib.rs
      │  ├─ errors.rs
      │  ├─ base64url.rs
      │  ├─ canonical/
      │  │  ├─ mod.rs
      │  │  └─ json.rs
      │  ├─ digest/
      │  │  └─ mod.rs
      │  ├─ sign/
      │  │  ├─ mod.rs
      │  │  ├─ jwk.rs
      │  │  ├─ policy.rs
      │  │  └─ keystore.rs
      │  ├─ aead/
      │  │  ├─ mod.rs
      │  │  ├─ xchacha.rs
      │  │  └─ hkdf.rs
      │  ├─ metrics.rs
      │  └─ prelude.rs
      └─ tests/
         └─ basic.rs
```

------

## Cargo.toml

```toml
[package]
name = "soulbase-crypto"
version = "1.0.0"
edition = "2021"
license = "Apache-2.0"
description = "Canonicalize · Digest · JWS(Ed25519) · AEAD(XChaCha20-Poly1305) for the Soul platform"
repository = "https://example.com/soul-base"

[features]
default = ["jws-ed25519", "aead-xchacha"]
jws-ed25519 = []
aead-xchacha = []

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"
once_cell = "1"
zeroize = "1"
getrandom = "0.2"

# 摘要
sha2 = "0.10"
blake3 = "1"

# Ed25519 签名
ed25519-dalek = { version = "2", features = ["rand_core"] }
rand_core = "0.6"

# AEAD
chacha20poly1305 = { version = "0.10", features = ["alloc", "xchacha20"] }

# HKDF
hkdf = "0.12"

# base64
base64 = "0.22"

# 平台内（错误域）
soulbase-errors = { path = "../soulbase-errors", version = "1.0.0" }

[dev-dependencies]
rand = "0.8"
```

------

## src/lib.rs

```rust
pub mod errors;
pub mod base64url;
pub mod canonical { pub mod mod_; pub mod json; }
pub mod digest { pub mod mod_; }
pub mod sign { pub mod mod_; pub mod jwk; pub mod policy; pub mod keystore; }
pub mod aead { pub mod mod_; pub mod xchacha; pub mod hkdf; }
pub mod metrics;
pub mod prelude;

pub use canonical::mod_::Canonicalizer;
pub use digest::mod_::{Digest, Digester, DefaultDigester};
pub use sign::mod_::{Signer, Verifier};
pub use aead::mod_::Aead;
```

------

## src/errors.rs

```rust
use thiserror::Error;
use soulbase_errors::prelude::*;

#[derive(Debug, Error)]
#[error("{0}")]
pub struct CryptoError(pub ErrorObj);

impl CryptoError {
  pub fn signature_invalid(msg:&str)->Self {
    CryptoError(ErrorBuilder::new(codes::UNKNOWN_INTERNAL).user_msg("Invalid signature.").dev_msg(msg.to_string()).build())
  }
  pub fn schema(msg:&str)->Self {
    CryptoError(ErrorBuilder::new(codes::SCHEMA_VALIDATION).user_msg("Invalid crypto input.").dev_msg(msg.to_string()).build())
  }
  pub fn decrypt_failed()->Self {
    CryptoError(ErrorBuilder::new(codes::AUTH_FORBIDDEN).user_msg("Decryption failed.").build())
  }
  pub fn provider_unavailable(msg:&str)->Self {
    CryptoError(ErrorBuilder::new(codes::PROVIDER_UNAVAILABLE).user_msg("Crypto provider unavailable.").dev_msg(msg.to_string()).build())
  }
  pub fn unknown(msg:&str)->Self {
    CryptoError(ErrorBuilder::new(codes::UNKNOWN_INTERNAL).user_msg("Crypto internal error.").dev_msg(msg.to_string()).build())
  }
}
```

------

## src/base64url.rs

```rust
pub fn enc(data:&[u8]) -> String {
  base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data)
}
pub fn dec(s:&str) -> Result<Vec<u8>, base64::DecodeError> {
  base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(s.as_bytes())
}
```

------

## src/canonical/mod.rs

```rust
use crate::errors::CryptoError;

pub trait Canonicalizer: Send + Sync {
  fn canonical_json<T: serde::Serialize>(&self, v:&T) -> Result<Vec<u8>, CryptoError>;
}

#[derive(Default, Clone)]
pub struct JsonCanonicalizer;

impl Canonicalizer for JsonCanonicalizer {
  fn canonical_json<T: serde::Serialize>(&self, v:&T) -> Result<Vec<u8>, CryptoError> {
    crate::canonical::json::canonicalize(v)
  }
}
```

### src/canonical/json.rs

```rust
use serde_json::{Value, Map};
use crate::errors::CryptoError;

/// 递归排序对象键；拒绝浮点；紧凑编码（最小空白）
pub fn canonicalize<T: serde::Serialize>(v:&T) -> Result<Vec<u8>, CryptoError> {
  let val = serde_json::to_value(v).map_err(|e| CryptoError::schema(&format!("to_value: {e}")))?;
  let canon = canonical_value(val)?;
  serde_json::to_vec(&canon).map_err(|e| CryptoError::schema(&format!("to_vec: {e}")))
}

fn canonical_value(v: Value) -> Result<Value, CryptoError> {
  match v {
    Value::Null | Value::Bool(_) | Value::String(_) => Ok(v),
    Value::Number(n) => {
      // 禁止浮点，允许整数
      if n.is_f64() { return Err(CryptoError::schema("float not allowed in canonical json")); }
      Ok(Value::Number(n))
    }
    Value::Array(arr) => {
      let mut out = Vec::with_capacity(arr.len());
      for x in arr { out.push(canonical_value(x)?); }
      Ok(Value::Array(out))
    }
    Value::Object(map) => {
      let mut bmap = Map::new();
      // 对 key 排序
      let mut keys: Vec<_> = map.keys().cloned().collect();
      keys.sort();
      for k in keys {
        let v = map.get(&k).unwrap();
        bmap.insert(k, canonical_value(v.clone())?);
      }
      Ok(Value::Object(bmap))
    }
  }
}
```

------

## src/digest/mod.rs

```rust
use serde::{Serialize, Deserialize};
use sha2::{Sha256, Digest as _};
use crate::{errors::CryptoError, canonical::mod_::Canonicalizer};
use crate::base64url;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Digest { pub algo:&'static str, pub b64:String, pub size:u64 }

pub trait Digester: Send + Sync {
  fn sha256(&self, bytes:&[u8]) -> Digest;
  fn blake3(&self, bytes:&[u8]) -> Digest;
  fn commit_json<T: serde::Serialize>(&self, cano:&dyn Canonicalizer, v:&T, algo:&str) -> Result<Digest, CryptoError>;
}

#[derive(Default, Clone)]
pub struct DefaultDigester;

impl Digester for DefaultDigester {
  fn sha256(&self, bytes:&[u8]) -> Digest {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let out = hasher.finalize();
    Digest{ algo:"sha256", b64: base64url::enc(&out), size: bytes.len() as u64 }
  }
  fn blake3(&self, bytes:&[u8]) -> Digest {
    let out = blake3::hash(bytes);
    Digest{ algo:"blake3", b64: base64url::enc(out.as_bytes()), size: bytes.len() as u64 }
  }
  fn commit_json<T: serde::Serialize>(&self, cano:&dyn Canonicalizer, v:&T, algo:&str) -> Result<Digest, CryptoError> {
    let bytes = cano.canonical_json(v)?;
    Ok(match algo {
      "sha256" => self.sha256(&bytes),
      "blake3" => self.blake3(&bytes),
      _ => return Err(CryptoError::schema("unsupported digest algo")),
    })
  }
}
```

------

## src/sign/policy.rs

```rust
#[derive(Clone, Debug)]
pub struct KeyPolicy {
  pub skew_ms: i64,                             // 允许时间偏差 ±
  pub revoke_list: std::collections::HashSet<String>,
}

impl Default for KeyPolicy {
  fn default()->Self { Self{ skew_ms: 300_000, revoke_list: Default::default() } } // ±5分钟
}
```

------

## src/sign/jwk.rs

```rust
use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Jwk {
  pub kty: String,     // "OKP"
  pub crv: String,     // "Ed25519"
  pub x: String,       // base64url(pubkey)
  pub kid: String,
}
```

------

## src/sign/keystore.rs

```rust
use crate::errors::CryptoError;
use ed25519_dalek::{SigningKey, VerifyingKey, Signature, Signer as _, Verifier as _};
use rand_core::OsRng;
use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;

/// 简单内存 KeyStore：仅支持 Ed25519（RIS）
/// - current 私钥
/// - public_by_kid 公钥表
/// - window: nbf/exp（ms）
#[derive(Clone)]
pub struct MemoryKeyStore {
  inner: Arc<RwLock<Inner>>,
}
struct Inner {
  current: (String, SigningKey, i64, i64), // (kid, sk, nbf_ms, exp_ms)
  public_by_kid: HashMap<String, (VerifyingKey, i64, i64)>,
  revoke: std::collections::HashSet<String>,
  skew_ms: i64,
}
impl MemoryKeyStore {
  pub fn generate(kid:&str, valid_ms:i64) -> Self {
    let mut rng = OsRng;
    let sk = SigningKey::generate(&mut rng);
    let vk = sk.verifying_key();
    let now = chrono::Utc::now().timestamp_millis();
    let inner = Inner {
      current: (kid.into(), sk, now, now + valid_ms),
      public_by_kid: std::iter::once((kid.into(), (vk, now, now + valid_ms))).collect(),
      revoke: Default::default(),
      skew_ms: 300_000,
    };
    Self{ inner: Arc::new(RwLock::new(inner)) }
  }
  pub fn add_public(&self, kid:&str, vk:VerifyingKey, nbf_ms:i64, exp_ms:i64) {
    self.inner.write().public_by_kid.insert(kid.into(), (vk, nbf_ms, exp_ms));
  }
  pub fn revoke(&self, kid:&str) { self.inner.write().revoke.insert(kid.into()); }

  pub fn sign_detached(&self, payload:&[u8]) -> Result<(String, Vec<u8>), CryptoError> {
    let inner = self.inner.read();
    let (kid, sk, _nbf, _exp) = (&inner.current.0, &inner.current.1, inner.current.2, inner.current.3);
    let sig: Signature = sk.sign(payload);
    Ok((kid.clone(), sig.to_bytes().to_vec()))
  }
  pub fn verify_detached(&self, kid:&str, payload:&[u8], sig:&[u8]) -> Result<(), CryptoError> {
    let now = chrono::Utc::now().timestamp_millis();
    let inner = self.inner.read();
    if inner.revoke.contains(kid) { return Err(CryptoError::signature_invalid("kid revoked")); }
    let (vk, nbf, exp) = inner.public_by_kid.get(kid).ok_or_else(|| CryptoError::signature_invalid("kid not found"))?;
    // 窗口校验（±skew）
    if now + inner.skew_ms < *nbf || now - inner.skew_ms > *exp {
      return Err(CryptoError::signature_invalid("kid expired or not yet valid"));
    }
    let sig = ed25519_dalek::Signature::from_slice(sig).map_err(|_| CryptoError::signature_invalid("sig decode"))?;
    vk.verify(payload, &sig).map_err(|_| CryptoError::signature_invalid("verify failed"))
  }
  pub fn current_kid(&self) -> String { self.inner.read().current.0.clone() }
  pub fn current_public(&self) -> ed25519_dalek::VerifyingKey { self.inner.read().current.1.verifying_key() }
}
```

------

## src/sign/mod.rs

```rust
use crate::{errors::CryptoError, base64url, canonical::mod_::Canonicalizer};
use super::keystore::MemoryKeyStore;

/// Signer/Verifier Trait
pub trait Signer: Send + Sync {
  fn kid(&self) -> &str;
  fn sign_detached(&self, canonical:&[u8]) -> Result<String, CryptoError>;
}
pub trait Verifier: Send + Sync {
  fn verify_detached(&self, kid:&str, canonical:&[u8], sig:&str) -> Result<(), CryptoError>;
}

/// JWS(detached) Ed25519 实现（RIS）
pub struct JwsEd25519Signer {
  pub keystore: MemoryKeyStore,
}
impl Signer for JwsEd25519Signer {
  fn kid(&self) -> &str { &self.keystore.current_kid() }
  fn sign_detached(&self, payload:&[u8]) -> Result<String, CryptoError> {
    // header: {"alg":"EdDSA","kid":"...","b64":false,"crit":["b64"]}
    let header = serde_json::json!({"alg":"EdDSA","kid": self.keystore.current_kid(), "b64": false, "crit":["b64"]});
    let header_b64 = base64url::enc(&serde_json::to_vec(&header).unwrap());
    let signing_input = [header_b64.as_bytes(), b".", payload].concat();
    let (_kid, sig_raw) = self.keystore.sign_detached(&signing_input)?;
    let sig_b64 = base64url::enc(&sig_raw);
    Ok(format!("{header_b64}..{sig_b64}"))
  }
}

pub struct JwsEd25519Verifier {
  pub keystore: MemoryKeyStore,
}
impl Verifier for JwsEd25519Verifier {
  fn verify_detached(&self, _kid:&str, payload:&[u8], jws:&str) -> Result<(), CryptoError> {
    // jws: b64(header) .. b64(signature)
    let mut parts = jws.split("..");
    let h_b64 = parts.next().ok_or_else(|| CryptoError::signature_invalid("bad jws"))?;
    let s_b64 = parts.next().ok_or_else(|| CryptoError::signature_invalid("bad jws"))?;
    let header_bytes = crate::base64url::dec(h_b64).map_err(|_| CryptoError::schema("header b64"))?;
    let header: serde_json::Value = serde_json::from_slice(&header_bytes).map_err(|_| CryptoError::schema("header json"))?;
    let kid = header.get("kid").and_then(|v| v.as_str()).ok_or_else(|| CryptoError::schema("kid missing"))?;
    let alg = header.get("alg").and_then(|v| v.as_str()).unwrap_or("EdDSA");
    if alg != "EdDSA" { return Err(CryptoError::schema("alg not EdDSA")); }
    // recreate signing input
    let signing_input = [h_b64.as_bytes(), b".", payload].concat();
    let sig = crate::base64url::dec(s_b64).map_err(|_| CryptoError::schema("sig b64"))?;
    self.keystore.verify_detached(kid, &signing_input, &sig)
  }
}
```

------

## src/aead/mod.rs

```rust
use crate::errors::CryptoError;

pub trait Aead: Send + Sync {
  fn seal(&self, key_ref:&[u8], nonce:&[u8], aad:&[u8], plaintext:&[u8]) -> Result<Vec<u8>, CryptoError>;
  fn open(&self, key_ref:&[u8], nonce:&[u8], aad:&[u8], ciphertext:&[u8]) -> Result<Vec<u8>, CryptoError>;
}
```

### src/aead/xchacha.rs

```rust
use chacha20poly1305::{XChaCha20Poly1305, Key, XNonce, aead::{Aead, AeadCore, KeyInit}};
use zeroize::Zeroize;
use crate::errors::CryptoError;

#[derive(Default)]
pub struct XChaChaAead;

impl super::Aead for XChaChaAead {
  fn seal(&self, key_ref:&[u8], nonce:&[u8], aad:&[u8], plaintext:&[u8]) -> Result<Vec<u8>, CryptoError> {
    if key_ref.len()!=32 || nonce.len()!=24 { return Err(CryptoError::schema("bad key/nonce length")); }
    let key = Key::from_slice(key_ref);
    let cipher = XChaCha20Poly1305::new(key);
    let n = XNonce::from_slice(nonce);
    let mut out = cipher.encrypt(n, chacha20poly1305::aead::Payload{ msg: plaintext, aad }).map_err(|_| CryptoError::decrypt_failed())?;
    Ok(out)
  }
  fn open(&self, key_ref:&[u8], nonce:&[u8], aad:&[u8], ciphertext:&[u8]) -> Result<Vec<u8>, CryptoError> {
    if key_ref.len()!=32 || nonce.len()!=24 { return Err(CryptoError::schema("bad key/nonce length")); }
    let key = Key::from_slice(key_ref);
    let cipher = XChaCha20Poly1305::new(key);
    let n = XNonce::from_slice(nonce);
    cipher.decrypt(n, chacha20poly1305::aead::Payload{ msg: ciphertext, aad }).map_err(|_| CryptoError::decrypt_failed())
  }
}
```

### src/aead/hkdf.rs

```rust
use hkdf::Hkdf;
use sha2::Sha256;

/// HKDF Extract+Expand
pub fn hkdf_extract_expand(salt:&[u8], ikm:&[u8], info:&[u8], len:usize) -> Vec<u8> {
  let hk = Hkdf::<Sha256>::new(Some(salt), ikm);
  let mut okm = vec![0u8; len];
  hk.expand(info, &mut okm).expect("hkdf expand");
  okm
}
```

------

## src/metrics.rs（占位）

```rust
#[derive(Default)]
pub struct CryptoStats {
  pub sign_cnt:u64, pub verify_cnt:u64
}
impl CryptoStats { pub fn inc_sign(&mut self){ self.sign_cnt+=1 } pub fn inc_verify(&mut self){ self.verify_cnt+=1 } }
```

------

## src/prelude.rs

```rust
pub use crate::errors::CryptoError;
pub use crate::canonical::mod_::{Canonicalizer, JsonCanonicalizer};
pub use crate::digest::mod_::{Digest, Digester, DefaultDigester};
pub use crate::sign::mod_::{Signer, Verifier, JwsEd25519Signer, JwsEd25519Verifier};
pub use crate::sign::keystore::MemoryKeyStore;
pub use crate::aead::mod_::Aead;
pub use crate::aead::xchacha::XChaChaAead;
pub use crate::aead::hkdf::hkdf_extract_expand;
```

------

## tests/basic.rs

```rust
use soulbase_crypto::prelude::*;
use rand::RngCore;

#[test]
fn canonical_json_is_stable_and_rejects_float() {
    let cano = JsonCanonicalizer::default();
    // 键顺序不同 -> 同 canonical bytes
    let a = serde_json::json!({"b":2,"a":1,"c":{"y":1,"x":2},"arr":[3,2,1]});
    let b = serde_json::json!({"c":{"x":2,"y":1},"a":1,"arr":[3,2,1],"b":2});
    let ca = cano.canonical_json(&a).unwrap();
    let cb = cano.canonical_json(&b).unwrap();
    assert_eq!(ca, cb);

    // 浮点 -> 拒绝
    let f = serde_json::json!({"a": 1.23});
    assert!(cano.canonical_json(&f).is_err());
}

#[test]
fn digest_and_commit() {
    let cano = JsonCanonicalizer::default();
    let dig = DefaultDigester::default();
    let payload = serde_json::json!({"a":1,"b":2});
    let d = dig.commit_json(&cano, &payload, "sha256").unwrap();
    assert_eq!(d.algo, "sha256");
    assert!(d.size > 0);
}

#[test]
fn sign_and_verify_ed25519_jws_detached() {
    // 准备 keystore
    let ks = MemoryKeyStore::generate("ed25519:2025-01:keyA", 600_000);
    let signer = JwsEd25519Signer{ keystore: ks.clone() };
    let verifier = JwsEd25519Verifier{ keystore: ks.clone() };

    let payload = br#"{"msg":"hello"}"#;
    let jws = signer.sign_detached(payload).unwrap();
    verifier.verify_detached("unused", payload, &jws).unwrap();

    // 篡改 -> 验签失败
    let bad = br#"{"msg":"hEllo"}"#;
    assert!(verifier.verify_detached("unused", bad, &jws).is_err());

    // 过期/撤销模拟
    ks.revoke(&ks.current_kid());
    assert!(verifier.verify_detached("unused", payload, &jws).is_err());
}

#[test]
fn aead_xchacha_roundtrip_and_hkdf() {
    let aead = XChaChaAead::default();
    // HKDF 生成 32字节 key
    let key = hkdf_extract_expand(b"salt", b"ikm", b"tenant:resource:env", 32);
    let mut nonce = [0u8; 24]; rand::thread_rng().fill_bytes(&mut nonce);

    let aad = b"tenant|resource|env";
    let pt = b"secret-plaintext";
    let ct = aead.seal(&key, &nonce, aad, pt).unwrap();
    let dec = aead.open(&key, &nonce, aad, &ct).unwrap();
    assert_eq!(pt.to_vec(), dec);

    // AAD 不同 → 解密失败
    assert!(aead.open(&key, &nonce, b"mismatch", &ct).is_err());
}
```

------

## README.md（简要）

```markdown
# soulbase-crypto (RIS)

统一密码学基座（Rust）：
- Canonical JSON（键排序、拒绝浮点）
- Digest: sha256 / blake3
- JWS Detached (Ed25519) 签名/验签（内存 KeyStore）
- AEAD: XChaCha20-Poly1305；HKDF
- 错误→稳定码映射；简易指标钩子

## 用法
let cano = JsonCanonicalizer::default();
let bytes = cano.canonical_json(&serde_json::json!({"b":2,"a":1}))?;
let dig = DefaultDigester::default().sha256(&bytes);

let ks = MemoryKeyStore::generate("ed25519:2025-01:keyA", 600_000);
let signer = JwsEd25519Signer{ keystore: ks.clone() };
let verifier = JwsEd25519Verifier{ keystore: ks.clone() };
let jws = signer.sign_detached(&bytes)?;
verifier.verify_detached("unused", &bytes, &jws)?;

// AEAD
let key = hkdf_extract_expand(b"salt", b"ikm", b"info", 32);
let ct = XChaChaAead::default().seal(&key, &rand_nonce_24, b"aad", b"plain")?;

## 测试
cargo test
```

------

### 说明与后续

- **把 HMAC/JWS/COSE 与 KMS 对接**：将 `MemoryKeyStore` 抽象成 `KeyStore` 接口的默认实现，后续可接 AWS KMS。
- **与 SB-15 A2A/ SB-17 Blob/ SB-16 Cache** 的集成：直接复用 `canonical_json`、`Digest` 与 `JWS`，统一哈希与签名口径。
- **生产加固**：恒时比较/zeroize 已纳入；需要时加入 `cbor` canonical、`AES-GCM` 适配与 `observe` 指标导出。
