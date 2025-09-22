### **文档 SB-18：soulbase-crypto（统一加密 · 签名 · 规范化序列化 / Crypto · Sign · Canonicalize）**

#### **第一部分：功能逻辑规约**

------

#### **0. 目标与范围（Objectives & Scope）**

- **目标**：为 Soul 生态提供**统一、可审计、最小披露**的**密码学基座**，为 A2A（跨域）、Evidence（证据）、幂等与摘要、对象加密（Blob）、缓存键规范化（Cache）等提供一致能力：
  1. **规范化序列化（Canonicalization）**：稳定字节表示，服务于签名/哈希/幂等键；
  2. **摘要/承诺（Digest/Commitment）**：`algo+b64+size` 的可验证摘要，贯穿证据与 A2A 收据链；
  3. **签名与验签（Sign/Verify）**：JWS（Detached）/COSE_Sign1，支持 Ed25519 与 ES256，含轮换与撤销；
  4. **对称加密（AEAD）**：统一 Envelope 加密（XChaCha20-Poly1305 / AES-GCM），支持 KMS/HKDF；
  5. **密钥生命周期**：KID 命名、JWKS/KeySet 装载、轮换与撤销表、时钟/有效期窗口校验；
  6. **安全随机与派生（RNG/HKDF）**：标准随机源与派生接口；
- **范围**：
  - 抽象：`Canonicalizer`、`Digester`、`Signer/Verifier`、`Aead`、`KeyStore/KeyResolver`、`KeyPolicy`；
  - 算法：`sha256/blake3`，`ed25519/es256`，`xchacha20-poly1305/aes-256-gcm`；
  - 与 `soulbase-a2a/observe/blob/cache/tx/qos` 的对接约定；
- **非目标**：不自研密码算法；不提供 PKI/CA 全流程（仅消费 JWKS/KMS）；不替代 TLS 信道安全。

------

#### **1. 功能定位（Functional Positioning）**

- **平台密码语义标准层**：统一序列化、摘要与签名封装，避免各模块“各选各的”导致验签/哈希不一致。
- **证据与互认的基础设施**：A2A 双签、Evidence 承诺、幂等锚与对象 ETag/加密一体化。
- **安全默认 + 可治理**：默认强算法与最小披露；可配置轮换窗口与撤销表；指标与错误规范化。

------

#### **2. 系统角色与地位（System Role & Position）**

- 层级：**基石层 / Soul-Base**；
- 关系：
  - **SB-15 A2A**：消息/收据的 canonical + JWS/COSE 签名与验签；
  - **SB-11 Observe**：Evidence 的 `Digest/Commitment`；
  - **SB-17 Blob**：Envelope 加密 / ETag 与副本一致性校验；
  - **SB-16 Cache**：canonical JSON → hash → cache key；
  - **SB-10/14**：`envelope_id` 生成、对账摘要一致、幂等锚哈希；
  - **SB-03 Config**：KeySet/JWKS/KMS 与轮换策略加载（快照哈希）。

------

#### **3. 核心概念与数据模型（Core Concepts & Data Model）**

- **Canonical JSON/CBOR**：字段排序、去 NaN/Infinity、数值规范、小数/整数判别、UTF-8 正规化（NFC），保证**相同语义 → 相同字节**；
- **Digest/Commitment**：`{algo:"sha256"| "blake3", b64:<base64url>, size:<u64>}`；
- **KeyMaterial**：`{kid, alg, use:"sig"|"enc", nbf, exp, jwk|cose}`；
- **KeySet（JWKS）**：公钥集；私钥仅在本域内由 `KeyStore` 管理（可封装 KMS）；
- **Signer/Verifier**：`{alg, kid}` + 轮换窗口（nbf/exp + 宽限）；
- **AEAD Envelope**：`{alg, nonce, aad, ciphertext, tag}`（明文不落盘；aad 写入 `tenant/resource/envelope_id` 等最小信息）；
- **KeyPolicy**：最小算法集、最小 key 长度、有效期与并行验证窗口、撤销列表。

------

#### **4. 不变式（Invariants）**

1. **稳定字节表示**：所有需签名/哈希/幂等/承诺的结构必须经 `Canonicalizer`；
2. **强算法默认**：`ed25519`/`es256`、`sha256`/`blake3`、`xchacha20-poly1305`/`aes-256-gcm`；
3. **轮换与撤销**：`kid` 必填；验签允许 `nbf/exp` 窗口内的新旧并行验证；撤销表即时生效；
4. **最小披露**：签名/加密接口只接受字节，不暴露秘钥/中间态；日志/Evidence 不写明文；
5. **恒定时序**：比较/验签/标签运算尽量恒时（避免侧信道）；
6. **RNG 标准化**：使用系统级 CSPRNG；禁止自定义伪随机；
7. **错误规范化**：`A2A.SIGNATURE_INVALID`、`SCHEMA.VALIDATION_FAILED`、`PROVIDER.UNAVAILABLE`、`UNKNOWN.INTERNAL` 等稳定码。

------

#### **5. 能力与抽象接口（Abilities & Abstract Interfaces）**

> 仅定义行为；具体 trait 在 TD/RIS 落地。

- **规范化序列化（Canonicalizer）**
  - `canonical_json<T:Serialize>(v) -> bytes`；
  - `canonical_cbor<T>(v) -> bytes`（可选）；
- **摘要/承诺（Digester）**
  - `digest(algo, bytes) -> Digest`；`commit(value:Serialize) -> Digest`；
- **签名（Signer）/验签（Verifier）**
  - `sign_detached(kid, bytes) -> jws|cose`；
  - `verify_detached(kid, bytes, sig) -> ()`；
  - `jws_compact` & `cose_sign1` 两种打包方式；
- **密钥管理（KeyStore/KeyResolver）**
  - `current(use="sig|enc") -> (kid, handle)`；
  - `resolve(kid) -> public`；
  - `rotate(next)`/`revoke(kid)`；
  - KMS（可选）包装：`sign_via_kms`/`aead_via_kms`；
- **AEAD（对称）**
  - `aead_seal(alg, key_ref, nonce, aad, plaintext) -> ciphertext+tag`；
  - `aead_open(alg, key_ref, nonce, aad, ciphertext+tag) -> plaintext`；
  - `hkdf_extract_expand` 提供密钥派生（绑定 `tenant/resource/envelope_id`）；
- **随机源（Rng）**
  - `fill_rand(&mut [u8])`、`nonce()`、`ulid()`（或暴露 helper）；
- **策略校验（KeyPolicy）**
  - `validate_keyset(jwks, policy)`；`window(now, nbf, exp, skew_ms)`；
- **观测（Observe hooks）**
  - `crypto_sign_ms`、`crypto_verify_ms`、`crypto_aead_ms`、`crypto_digest_total{algo}`。

------

#### **6. 指标与 SLO / 验收（Metrics, SLOs & Acceptance）**

- **SLO**：
  - `sign_detached` p95 ≤ **2ms**（Ed25519 本地）；`verify_detached` p95 ≤ **5ms**；
  - `canonical_json` p95 ≤ **0.2ms**（中等对象）；`aead_seal/open` p95 ≤ **1ms**（64KB）；
  - 轮换切换期**零误判**（新旧 key 并行验证）。
- **验收**：
  - 契约：跨模块 hash/签名一致性；撤销/过期/偏时窗校验；
  - 基准：批量签名/验签吞吐与尾延；
  - 混沌：Key 轮换/撤销/时间偏差 ±300s；KMS 不可用时的降级/重试策略。

------

#### **7. 依赖与边界（Dependencies & Boundaries）**

- **上游**：A2A/Observe/Blob/Cache/Tx/QoS/Config；
- **下游**：系统 RNG、KMS/JWKS（Auth）、本地或 HSM；
- **边界**：不提供 CA/证书签发；不承担网络传输安全（TLS 仍由网关/客户端负责）。

------

#### **8. 风险与控制（Risks & Controls）**

- **多实现不一致** → **强制 Canonicalizer**；在 CI 的 Contract-TestKit 增加跨 crate 比对用例；
- **旧 Key 拒绝/新 Key 未发布** → 轮换宽限窗与双验证；
- **私钥泄露** → 仅通过 `KeyStore`/KMS 使用；不暴露私钥字节；默认禁止将私钥加载为字符串；
- **随机源退化** → 仅允许标准 CSPRNG（`getrandom`），失败上报并熔断敏感操作；
- **侧信道** → 用常数时间比较；避免日志写入敏感值/密钥片段。

------

#### **9. 关键交互序列（Key Interaction Sequences）**

**9.1 A2A 签名与验签**

1. 构造消息头体，`canonical_json` → bytes；
2. `sign_detached(kid, bytes)` → jws；发送带 `kid`+`sig` 的消息；
3. 接收端 `resolve(kid)`、`verify_detached(kid, bytes, sig)` → 通过；
4. 轮换期：新旧 `kid` 均在 JWKS，验证均通过；过期后旧 `kid` 立即失效。

**9.2 Evidence 承诺与幂等**

1. 对对象/响应 `commit(value)` → `Digest{algo,b64,size}`；
2. Evidence/账页/收据链仅存 `Digest` 与 `BlobRef`；
3. SB-10/14 以 `envelope_id` 或 `digest.b64` 作为幂等锚辅助键。

**9.3 Blob Envelope 加密**

1. 通过 KMS 获取/解封主密钥 → HKDF(tenant, resource, envelope) → `k_enc`；
2. `aead_seal` 生成密文，AAD 含 `tenant/resource/envelope_id`；
3. 解密需同样 AAD 与 nonce；AAD 不匹配 → 恒时失败。

------

#### **10. 开放问题（Open Issues / TODO）**

- **JWS vs COSE 的优先级**：默认 JWS（JSON 系栈），对 IoT/嵌入式可加 COSE；
- **KMS/HSM 多厂商**：AWS KMS / GCP KMS / 本地 HSM 的统一适配（签名代理与 AEAD 代理）；
- **Post-Quantum 预留**：是否提供 PQC 算法占位（Dilithium 等）的接口兼容层；
- **跨语言 SDK 一致性**：TS/Go/Java 的 canonical/签名保持一致（Contract-TestKit 增加互操作用例）。

------

> 若你认可该规约，我将按“三件套”继续输出 **SB-18-TD（技术设计已完成 → RIS）** 的**最小可运行骨架**：
>
> - `canonical_json`（稳定字段排序）与 `sha256/blake3` 摘要；
> - `ed25519` JWS-Detached 的 Sign/Verify（本地内存 KeyStore）；
> - `xchacha20-poly1305` 的 AEAD 封装；
> - 单测：跨模块 canonical 一致性、签名/验签、AEAD seal/open、撤销/过期窗口校验。
