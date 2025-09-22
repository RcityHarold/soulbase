# RIS 示例（JWS Detached 签名/验签 + Canonical）

> 目标：演示使用 Canonical JSON 与 Ed25519 JWS Detached 对 A2A 消息签名与验签；与 Issue 12 对齐。

```rust
use soulbase_crypto::prelude::*;
use serde_json::json;

fn demo() -> Result<(), CryptoError> {
    // 1) Canonical JSON
    let cano = soulbase_crypto::canonical::JsonCanonicalizer::default();
    let payload = json!({"envelope_id":"env_v7","seq":42,"ts":1730000000});
    let bytes = cano.canonical_json(&payload)?;

    // 2) Sign (JWS Detached, Ed25519)
    let kid = "ed25519:2025-01:keyA";
    let signer = soulbase_crypto::sign::JwsEd25519::from_insecure_dev_key(kid)?; // RIS 示例：开发用 key
    let jws = signer.sign_detached(kid, &bytes)?;

    // 3) Verify
    let verifier = signer.verifier();
    verifier.verify_detached(kid, &bytes, &jws)?;
    Ok(())
}
```

要点：
- `canonical_json` 确保摘要与签名跨端一致；
- 证据记录 `kid/alg/payload_digest` 与 `envelope_id`；
- 验签需恒时比较，撤销与有效期校验留给 KeyStore。
