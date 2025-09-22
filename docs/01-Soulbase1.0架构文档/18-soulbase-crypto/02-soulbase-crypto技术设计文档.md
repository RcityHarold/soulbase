# 文档 SB-18-TD：`soulbase-crypto` 技术设计

（Canonicalize · Digest · JWS/COSE Sign/Verify · AEAD · KeyStore · HKDF · Observe）

> 对应规约：SB-18
>  目标：给出**可落地**的 Rust 设计：模块结构、Trait/API、规范化序列化规则、摘要/承诺、JWS/COSE 签名与验签、AEAD 封装、密钥轮换/撤销、观测与错误映射；与 `soulbase-a2a/observe/blob/cache/tx/qos` 的集成位。
>  说明：本 TD 只含接口/规则，不包含实现代码；RIS 下一步提供最小可运行骨架。

------

## 1. Crate 结构与 Feature 规划

```
soulbase-crypto/
  src/
    lib.rs
    errors.rs              # CryptoError → SB-02 错误映射 (A2A.SIGNATURE_INVALID / SCHEMA.VALIDATION_FAILED / PROVIDER.UNAVAILABLE / UNKNOWN.INTERNAL)
    base64url.rs           # 无填充 base64url 编解码
    canonical/
      mod.rs               # Canonicalizer (JSON/CBOR)：稳定字节
      json.rs              # JSON Canonicalization (JCS-like, RFC8785子集)
      cbor.rs              # 可选；默认关闭
    digest/
      mod.rs               # Digester (sha256|blake3) & Digest类型
    sign/
      mod.rs               # Signer/Verifier Trait；JWS/COSE facade
      jws.rs               # JWS(detached) Ed25519/ES256
      cose.rs              # COSE_Sign1（可选）
      jwk.rs               # JWK/JWKS 解析与 KID 规范
      policy.rs            # KeyPolicy（有效期/算法/轮换窗口/撤销表）
      keystore.rs          # KeyStore & KeyResolver（本地/KMS/JWKS）
    aead/
      mod.rs               # AEAD统一接口 (seal/open)
      xchacha.rs           # XChaCha20-Poly1305
      aesgcm.rs            # AES-256-GCM
      hkdf.rs              # HKDF(Extract+Expand)
      zero.rs              # 零化工具 (zeroize) & 常数时间比较
    metrics.rs             # SB-11 指标导出钩子
    prelude.rs
```

**features**

- `jws-ed25519`（默认）/`jws-es256`/`cose`
- `aead-xchacha`（默认）/`aead-aesgcm`
- `canonical-cbor`（默认只开 JSON）
- `kms`（KeyStore 的 KMS 代理）
- `observe`（导出指标）

------

## 2. 数据类型与模型

```rust
/// 承诺/摘要（证据/A2A/幂等）
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Digest { pub algo: &'static str, pub b64: String, pub size: u64 }

/// 密钥材料（公钥为主；私钥通过 KeyStore 访问）
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct KeyMaterial {
  pub kid: String,              // "ed25519:2025-01:keyA" 规范见 §7
  pub alg: KeyAlg,              // Sig: Ed25519|ES256；Enc: Aes256Gcm|XChaCha20Poly1305
  pub use_: KeyUse,             // Sig | Enc
  pub nbf_ms: i64,              // not-before (UTC ms)
  pub exp_ms: i64,              // not-after  (UTC ms)
  pub jwk_or_cose: serde_json::Value, // 公钥 (JWK/COSE Key)；私钥不落盘
  pub fingerprint: String,      // 公钥指纹（base64url(sha256(jwk))）
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub enum KeyUse { Sig, Enc }
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub enum KeyAlg { Ed25519, Es256, Aes256Gcm, XChaCha20Poly1305 }

/// Key 策略（轮换/撤销/允许算法）
#[derive(Clone, Debug)]
pub struct KeyPolicy {
  pub allowed_sig_algs: Vec<KeyAlg>,     // [Ed25519, Es256]
  pub allowed_enc_algs: Vec<KeyAlg>,     // [XChaCha20Poly1305, Aes256Gcm]
  pub skew_ms: i64,                       // 时间偏差容忍 ±
  pub revoke_list: std::collections::HashSet<String>, // KID 列表
}
```

------

## 3. 规范化序列化（Canonicalization，JSON/CBOR）

### 3.1 规则（JSON 子集，参考 RFC 8785 “JCS”，结合工程约束）

- **编码**：UTF-8；对象**键必须排序**（字典序，按 Unicode 码点）；数组**保持原序**；
- **数值**：整数以最短十进制表示；浮点不允许 NaN/±Inf（调用方需在上层约束/转换）；
- **布尔/Null**：转为 `true/false/null`；
- **字符串**：不转义非必要字符，保持 NFC 归一化（预处理）；
- **空白**：输出最小 JSON，无多余空格/换行；
- **禁止**：尾随小数点、前导零（除 `0` 本身）。

### 3.2 API（`canonical/mod.rs`）

```rust
pub trait Canonicalizer: Send + Sync {
  fn canonical_json<T: serde::Serialize>(&self, v:&T) -> Result<Vec<u8>, CryptoError>;
  fn canonical_cbor<T: serde::Serialize>(&self, v:&T) -> Result<Vec<u8>, CryptoError>; // feature
}
```

**实现要点**

- `serde_json::Value` → 递归排序 `Object` 键（`BTreeMap`）→ 按最小格式 encode。
- 浮点：默认**拒绝**（返回 `SCHEMA.VALIDATION_FAILED`），如业务需要，可提供 `float_policy: Strict|LosslessString`。
- 提供 `canonical_bytes_hash(v) -> (bytes, Digest)` 便捷函数。

------

## 4. 摘要/承诺（Digester）

```rust
pub trait Digester: Send + Sync {
  fn sha256(&self, bytes:&[u8]) -> Digest;   // algo="sha256"
  fn blake3(&self, bytes:&[u8]) -> Digest;   // algo="blake3"
  fn commit_json<T: serde::Serialize>(&self, cano:&dyn Canonicalizer, v:&T, algo:&str) -> Result<Digest, CryptoError>;
}
```

- `commit_json` = `canonical_json(v)` 后做 `sha256|blake3`。
- Base64 编码使用无填充 base64url。
- 用途：A2A 承诺、Evidence、幂等锚、Blob ETag 对齐等。

------

## 5. 签名与验签（JWS/COSE）

### 5.1 Trait（`sign/mod.rs`）

```rust
pub trait Signer: Send + Sync {
  fn kid(&self) -> &str;
  /// 对 canonical bytes 做 detached 签名，返回 JWS/COSE 字符串
  fn sign_detached(&self, alg: KeyAlg, canonical:&[u8]) -> Result<String, CryptoError>;
}

pub trait Verifier: Send + Sync {
  /// 验证 detached 签名；根据 KID 解析公钥；校验 nbf/exp/skew 与撤销表
  fn verify_detached(&self, alg: KeyAlg, kid:&str, canonical:&[u8], sig:&str) -> Result<(), CryptoError>;
}
```

### 5.2 JWS 细节（`sign/jws.rs`）

- **JOSE Header（base64url）**：`{"alg":"EdDSA"|"ES256","kid":"...","b64":false,"crit":["b64"]}`（Detached：`b64=false`）。
- 签名输入：`ASCII(BASE64URL(header)) || "." || payload_bytes`；
- 签名输出：`BASE64URL(signature)`；返回 `BASE64URL(header) + ".." + BASE64URL(signature)`（无 payload）；
- **验签**：按 `kid` 找公钥 → 校验 `nbf/exp/skew` 与撤销表；失败映射 `A2A.SIGNATURE_INVALID`。

> **注意**：A2A 场景首推 JWS；IoT/嵌入式可按需启用 COSE_Sign1（`sign/cose.rs`）。

------

## 6. KeyStore / KeyResolver / 轮换与撤销

```rust
pub trait KeyStore: Send + Sync {
  fn current_sig(&self) -> Result<KeyMaterial, CryptoError>;             // 当前签名私钥（封装 handle，不暴露原始私钥）
  fn current_enc(&self) -> Result<KeyMaterial, CryptoError>;             // 当前加密密钥（或 KMS handle）
  fn rotate(&self, next: KeyMaterial) -> Result<(), CryptoError>;        // 切换新KID
  fn revoke(&self, kid:&str) -> Result<(), CryptoError>;
}

pub trait KeyResolver: Send + Sync {
  fn resolve_public(&self, kid:&str) -> Result<KeyMaterial, CryptoError>; // 解析公钥（JWKS）
  fn policy(&self) -> &KeyPolicy;
}
```

**轮换策略**

- 新旧 key **并行验证**窗口：`[new.nbf - skew, old.exp + skew]`；
- 签名始终使用 `current_sig().kid`；
- 撤销：`revoke_list` 即时生效 → `verify_detached` 直接拒绝。

**KID 规范**

- 推荐 `"ed25519:YYYYMM:keyAlias"` / `"es256:YYYYMM:keyAlias"`；确保**全局唯一**与**时间含义**；便于调试与对账。

------

## 7. AEAD 与 HKDF

### 7.1 AEAD 统一接口（`aead/mod.rs`）

```rust
pub trait Aead: Send + Sync {
  fn seal(&self, alg: KeyAlg, key_ref:&str, nonce:&[u8], aad:&[u8], plaintext:&[u8]) -> Result<Vec<u8>, CryptoError>;
  fn open(&self, alg: KeyAlg, key_ref:&str, nonce:&[u8], aad:&[u8], ciphertext:&[u8]) -> Result<Vec<u8>, CryptoError>;
}
```

- **alg**：`XChaCha20Poly1305`（默认）或 `Aes256Gcm`；
- `key_ref`：来自 `KeyStore.current_enc().kid` 或 KMS 别名；
- **Nonce**：随机（`xchacha` 24 bytes / `aesgcm` 12 bytes），由调用方生成；
- **AAD**：建议绑定 `tenant|resource|envelope_id|policy_hash` 等最小上下文，解密时必须一致。
- **恒时比较与零化**：内部使用常数时间比较，密钥材料用 `zeroize` 擦除。

### 7.2 HKDF（`aead/hkdf.rs`）

- `hkdf_extract_expand(salt, ikm, info, len) -> key_bytes`；
- 用于把**主密钥**与**业务上下文**绑定派生：例如 `ikm=master_key`, `info=b"tenant:resource:env_id"`；
- 与 `Blob` & `A2A` 结合：对象 Envelope 加密/对等协商密钥。

------

## 8. 错误映射（`errors.rs`）

| 场景                             | 稳定码（SB-02）                                    |
| -------------------------------- | -------------------------------------------------- |
| 验签失败/撤销/过期/算法不允许    | `A2A.SIGNATURE_INVALID`（建议在 SB-02 新增）       |
| 规范化失败/不支持的数值/非法输入 | `SCHEMA.VALIDATION_FAILED`                         |
| KMS/KeyStore 不可用              | `PROVIDER.UNAVAILABLE`                             |
| AEAD 解密失败                    | `AUTH.FORBIDDEN`（或新增 `CRYPTO.DECRYPT_FAILED`） |
| 未分类                           | `UNKNOWN.INTERNAL`                                 |

**公共视图**：只返回 `code + message`；绝不输出 key/nonce/plaintext/sig 原文；诊断细节进 Evidence（最小化）。

------

## 9. 指标与观测（`metrics.rs`）

- `crypto_sign_ms_bucket{alg}` / `crypto_verify_ms_bucket{alg}`
- `crypto_digest_total{algo}`
- `crypto_aead_ms_bucket{alg,op=seal|open}`
- `crypto_key_rotate_total` / `crypto_key_revoke_total`
- 标签最小集：`tenant`（若有关联）、`alg`、`kid`（可匿名化/哈希）

------

## 10. 与周边模块的对接

- **A2A（SB-15）**：
  - 发送：`canonical_json(msg)` → `sign_detached(kid, bytes)` → JWS；
  - 接收：`verify_detached`（新旧 KID 并行验证 + 撤销表）。
- **Observe（SB-11）**：
  - Evidence `Digest` 用 `commit_json(value)`；
  - 指标：签名/验签/AEAD 时延；错误码一致性。
- **Blob（SB-17）**：
  - Envelope 加密：`hkdf(tenant,resource,envelope)`→`aead_seal/open`；
  - `Digest` 与 ETag 对齐（sha256）。
- **Cache（SB-16）**：
  - `canonical_json(payload)`→`sha256`→ cache key hash；
  - 保障所有模块同一 canonical 规则产生**一致键**。
- **Tx/QoS（SB-10/14）**：
  - `envelope_id` 生成可用 `blake3(canonical_json(envelope_header))`；
  - 对账摘要与幂等锚统一。

------

## 11. 安全注意事项

- **常数时间比较**：所有 MAC/tag 比较与签名验证必须用恒时函数；
- **随机源**：`getrandom`；失败则**拒绝**敏感操作并上报告警；
- **私钥保护**：Sign 通过 `KeyStore`/KMS 句柄，**不返回私钥**；
- **日志/ Evidence**：禁止记录私钥/原文；只记录 `kid/alg/digest/len` 摘要；
- **轮换窗口**：默认 ±300s `skew_ms`；可配置；
- **撤销表**：内存+持久化；热更即时生效。

------

## 12. 测试与验收

- **契约**：
  - 不同模块对同一结构 `canonical_json` 字节一致；
  - JWS 签名/验签成功；修改任意字节 → 验签失败；
  - 轮换：旧/新 KID 并行验证通过；过期后旧 KID 失败；
  - 撤销表命中 → 失败；
  - AEAD：`seal`→`open` 还原一致；AAD 不同 → 失败；
  - HKDF：相同 `ikm/salt/info` → 相同输出；任一变更 → 不同输出。
- **基准**：
  - `sign/verify/aead/canonical` p50/p95 时延；
  - 大对象（>64KB） AEAD 测试。
- **混沌**：
  - 模拟时间偏移 ±600s；KMS 不可用时重试/降级行为。

------

## 13. RIS（下一步实现计划）

- `canonical/json.rs`：JCS 子集实现（对象键排序/紧凑编码/拒绝浮点）。
- `digest/mod.rs`：`sha256`/`blake3` + base64url 无填充。
- `sign/jws.rs`：Ed25519（默认）与 ES256（可选）；`KeyStore` 本地内存 & JWKS 解析。
- `aead/xchacha.rs` 与 `hkdf.rs`：`XChaCha20-Poly1305` 与 HKDF；零化与恒时比较工具（`zeroize`）。
- 指标钩子：`observe` feature 对接 SB-11。
- 单测：canonical 一致性、签名/验签、AEAD、轮换/撤销。

------

### 小结

本技术设计确定了 `soulbase-crypto` 的**稳定接口与强规则**：

- 一处定义“**如何序列化/如何哈希/如何签名/如何加密**”；
- 可被 A2A/Observe/Blob/Cache/Tx/QoS **直接复用**；
- 以**安全默认 + 轮换撤销 + 观测指标**为基线，满足生产级需求。

若确认无误，我将输出 **SB-18-RIS（最小可运行骨架）**：包含 JSON Canonicalizer、sha256/blake3 摘要、Ed25519 JWS-Detached、XChaCha20-Poly1305 AEAD、KeyStore 本地内存与基本单测。
