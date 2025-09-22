# RIS 示例（签名 + 反重放窗口）

> 目标：演示 A2A 请求签名与简单反重放窗口校验；与 Issue 12 对齐（真实实现交由存储/Tx）。

```rust
use std::collections::BTreeMap;
use soulbase_crypto::prelude::*;
use serde_json::json;

struct ReplayWindow {
  // 简化：env_id -> last_seq
  last: BTreeMap<String, i64>,
}
impl ReplayWindow {
  fn new() -> Self { Self{ last: BTreeMap::new() } }
  fn accept(&mut self, env_id:&str, seq:i64) -> bool {
    let ok = match self.last.get(env_id) { Some(&s) => seq > s, None => true };
    if ok { self.last.insert(env_id.to_string(), seq); }
    ok
  }
}

fn sign_request(env:&str, seq:i64) -> Result<(String,String), CryptoError> {
  let cano = soulbase_crypto::canonical::JsonCanonicalizer::default();
  let payload = json!({"envelope_id":env, "seq":seq});
  let bytes = cano.canonical_json(&payload)?;
  let kid = "ed25519:2025-01:keyA";
  let signer = soulbase_crypto::sign::JwsEd25519::from_insecure_dev_key(kid)?;
  let jws = signer.sign_detached(kid, &bytes)?;
  Ok((jws, base64::encode(bytes)))
}

fn verify_and_check(env:&str, seq:i64, jws:&str, bytes:&[u8], win:&mut ReplayWindow) -> Result<(), &'static str> {
  let verifier = soulbase_crypto::sign::JwsEd25519::from_insecure_dev_key("ed25519:2025-01:keyA").unwrap().verifier();
  verifier.verify_detached("ed25519:2025-01:keyA", bytes, jws).map_err(|_| "A2A.SIGNATURE_INVALID")?;
  if !win.accept(env, seq) { return Err("A2A.REPLAY"); }
  Ok(())
}
```

要点：
- `ReplayWindow` 仅为演示；实际应使用持久化存储 + 过期窗口；
- 以 `envelope_id` 为主锚，`seq` 为辅助；
- 验证失败映射到 SB‑02 的稳定码。
