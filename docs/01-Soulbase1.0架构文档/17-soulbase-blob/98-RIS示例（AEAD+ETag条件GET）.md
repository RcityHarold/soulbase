# RIS 示例（AEAD + ETag 条件 GET）

> 目标：演示 AEAD/HKDF 加密上下文与 ETag=sha256 的条件 GET；与 Issue 08 对齐。

```rust
use soulbase_crypto::prelude::*;
use sha2::{Digest as ShaDigest, Sha256};

fn aead_roundtrip(tenant:&str, bucket:&str, key:&str, env:&str, plaintext:&[u8]) -> Result<(), CryptoError> {
  // 1) HKDF 派生（与 SB‑17 绑定一致）
  let hk = soulbase_crypto::aead::hkdf::HkdfEngine::default();
  let info = format!("blob:{}/{}", bucket, key);
  let k = hk.derive_key(tenant.as_bytes(), env.as_bytes(), info.as_bytes());

  // 2) AEAD 加密
  let aead = soulbase_crypto::aead::xchacha::XChaCha20Poly1305Cipher::new(&k);
  let aad = format!("{}/{}/{}", tenant, bucket, key);
  let ct = aead.seal(plaintext, aad.as_bytes())?;

  // 3) AEAD 解密
  let pt = aead.open(&ct, aad.as_bytes())?;
  assert_eq!(pt, plaintext);
  Ok(())
}

fn etag_sha256(body:&[u8]) -> String {
  let mut hasher = Sha256::new();
  hasher.update(body);
  let hex = hex::encode(hasher.finalize());
  format!("\"{}\"", hex) // S3 风格引号
}
```

要点：
- 派生：`ikm=tenant key`、`salt=envelope_id`、`info=blob:{bucket}/{key}`；
- ETag 使用 sha256；Presign/GET 可用 `If-None-Match` 条件请求；
- 日志/Evidence 仅记录 `BlobRef+Digest` 与 `alg/aad_digest`，不含原文。
