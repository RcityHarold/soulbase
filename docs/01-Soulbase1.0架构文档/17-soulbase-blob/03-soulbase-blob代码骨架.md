下面是 **SB-17-RIS：`soulbase-blob` 最小可运行骨架**。
 与 SB-17（规约）& SB-17-TD（设计）一致，骨架提供：

- 统一 DTO：`BlobRef / BlobMeta / Digest / PutOpts / GetOpts / Presign* / Multipart*（占位）`
- 抽象 Trait：`BlobStore`、`RetentionExec`
- **本地 FS 适配**（开发态）：原子写、ETag = sha256、私有默认、简易 Presign（HMAC + 过期）
- S3/MinIO 适配 **stub**（接口占位，便于后续接入）
- Retention 执行器：按 `{tenant}/{namespace}/...` 前缀 + TTL 删除，写处理计数
- 端到端单测：**上传→读取→删除**、**Presign 过期**、**Retention 立即生效**

> 放到 `soul-base/crates/soulbase-blob/` 后，执行 `cargo check && cargo test`。

------

## 目录结构

```
soul-base/
└─ crates/
   └─ soulbase-blob/
      ├─ Cargo.toml
      ├─ README.md
      ├─ src/
      │  ├─ lib.rs
      │  ├─ errors.rs
      │  ├─ model.rs
      │  ├─ key.rs
      │  ├─ trait.rs
      │  ├─ policy.rs
      │  ├─ retry.rs
      │  ├─ metrics.rs
      │  ├─ s3/
      │  │  └─ mod.rs
      │  ├─ fs/
      │  │  ├─ mod.rs
      │  │  └─ presign.rs
      │  ├─ retention/
      │  │  └─ mod.rs
      │  └─ prelude.rs
      └─ tests/
         └─ e2e.rs
```

------

## Cargo.toml

```toml
[package]
name = "soulbase-blob"
version = "1.0.0"
edition = "2021"
license = "Apache-2.0"
description = "Objects & Artifacts Store (S3/MinIO/FS) for the Soul platform"
repository = "https://example.com/soul-base"

[features]
default = ["backend-fs"]
backend-fs = []
backend-s3 = []     # 预留：接入 S3/MinIO SDK
kms = []
observe = []
qos = []

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
async-trait = "0.1"
thiserror = "1"
bytes = "1"
chrono = "0.4"
sha2 = "0.10"
base64 = "0.22"
hmac = "0.12"
urlencoding = "2.1.3"

# 平台内
soulbase-errors = { path = "../soulbase-errors", version = "1.0.0" }

[dev-dependencies]
tokio = { version = "1", features = ["rt-multi-thread","macros","time","fs"] }
tempfile = "3"
```

------

## src/lib.rs

```rust
pub mod errors;
pub mod model;
pub mod key;
pub mod r#trait;
pub mod policy;
pub mod retry;
pub mod metrics;
pub mod s3 { pub mod mod_; }
pub mod fs { pub mod mod_; pub mod presign; }
pub mod retention { pub mod mod_; }
pub mod prelude;

pub use r#trait::{BlobStore, RetentionExec};
pub use model::*;
pub use fs::mod_::FsBlobStore;
```

------

## src/errors.rs

```rust
use thiserror::Error;
use soulbase_errors::prelude::*;

#[derive(Debug, Error)]
#[error("{0}")]
pub struct BlobError(pub ErrorObj);

impl BlobError {
  pub fn provider_unavailable(msg:&str)->Self {
    BlobError(ErrorBuilder::new(codes::PROVIDER_UNAVAILABLE)
      .user_msg("Blob backend unavailable.").dev_msg(msg.to_string()).build())
  }
  pub fn not_found(msg:&str)->Self {
    BlobError(ErrorBuilder::new(codes::STORAGE_NOT_FOUND)
      .user_msg("Object not found.").dev_msg(msg.to_string()).build())
  }
  pub fn forbidden(msg:&str)->Self {
    BlobError(ErrorBuilder::new(codes::AUTH_FORBIDDEN)
      .user_msg("Access denied.").dev_msg(msg.to_string()).build())
  }
  pub fn schema(msg:&str)->Self {
    BlobError(ErrorBuilder::new(codes::SCHEMA_VALIDATION)
      .user_msg("Invalid blob request.").dev_msg(msg.to_string()).build())
  }
  pub fn unknown(msg:&str)->Self {
    BlobError(ErrorBuilder::new(codes::UNKNOWN_INTERNAL)
      .user_msg("Internal blob error.").dev_msg(msg.to_string()).build())
  }
}
```

------

## src/model.rs

```rust
use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlobRef {
  pub bucket: String,
  pub key: String,
  pub etag: String,
  pub size: u64,
  pub content_type: String,
  pub created_at_ms: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlobMeta {
  pub ref_: BlobRef,
  pub md5_b64: Option<String>,
  pub user_tags: Option<std::collections::BTreeMap<String,String>>,
  pub storage_class: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Digest { pub algo:&'static str, pub b64:String, pub size:u64 }

#[derive(Clone, Debug, Default)]
pub struct PutOpts {
  pub content_type: Option<String>,
  pub ttl_days: Option<u32>,
  pub encrypt: bool,
  pub user_tags: Option<std::collections::BTreeMap<String,String>>,
  pub envelope_id: Option<String>, // 幂等锚（记录到元数据文件）
}

#[derive(Clone, Debug, Default)]
pub struct GetOpts {
  pub range: Option<(u64,u64)>,
  pub if_none_match: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct PresignGetOpts { pub expire_secs: u32 }

#[derive(Clone, Debug, Default)]
pub struct PresignPutOpts {
  pub expire_secs: u32,
  pub content_type: Option<String>,
  pub size_hint: Option<u64>,
}

/*** Multipart（骨架占位） ***/
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MultipartInit { pub upload_id:String, pub ref_hint:BlobRef }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PartETag { pub part_number:u32, pub etag:String, pub size:u64 }
#[derive(Clone, Debug)]
pub struct MultipartPutOpts { pub content_type:Option<String>, pub encrypt:bool, pub envelope_id:Option<String> }
```

------

## src/key.rs

```rust
pub fn ensure_key(tenant:&str, key:&str) -> Result<(), String> {
  if !key.starts_with(&format!("{tenant}/")) { return Err("key must start with tenant/".into()); }
  if key.contains("..") || key.starts_with('/') || key.contains('\\') { return Err("invalid key path".into()); }
  Ok(())
}
```

------

## src/trait.rs

```rust
use crate::{errors::BlobError, model::*};
use bytes::Bytes;
use futures_core::Stream;

#[async_trait::async_trait]
pub trait BlobStore: Send + Sync {
  async fn put(&self, bucket:&str, key:&str, body:Bytes, opts:PutOpts) -> Result<BlobRef, BlobError>;
  async fn put_stream<S>(&self, bucket:&str, key:&str, stream:S, content_len:Option<u64>, opts:PutOpts) -> Result<BlobRef, BlobError>
  where S: Stream<Item = Result<Bytes, BlobError>> + Send + Unpin + 'static;

  async fn get(&self, bucket:&str, key:&str, opts:GetOpts) -> Result<Bytes, BlobError>;
  async fn head(&self, bucket:&str, key:&str) -> Result<BlobMeta, BlobError>;
  async fn delete(&self, bucket:&str, key:&str) -> Result<(), BlobError>;

  async fn presign_get(&self, bucket:&str, key:&str, opts:PresignGetOpts) -> Result<String, BlobError>;
  async fn presign_put(&self, bucket:&str, key:&str, opts:PresignPutOpts) -> Result<String, BlobError>;

  async fn multipart_begin(&self, bucket:&str, key:&str, opts:MultipartPutOpts) -> Result<MultipartInit, BlobError>;
  async fn multipart_put_part(&self, _bucket:&str, _key:&str, _upload_id:&str, _part_number:u32, _bytes:Bytes) -> Result<PartETag, BlobError> {
    Err(BlobError::unknown("multipart not implemented in FS adapter"))
  }
  async fn multipart_complete(&self, _bucket:&str, _key:&str, _upload_id:&str, _parts:Vec<PartETag>) -> Result<BlobRef, BlobError> {
    Err(BlobError::unknown("multipart not implemented in FS adapter"))
  }
  async fn multipart_abort(&self, _bucket:&str, _key:&str, _upload_id:&str) -> Result<(), BlobError> { Ok(()) }
}

#[async_trait::async_trait]
pub trait RetentionExec: Send + Sync {
  async fn apply_rule(&self, rule:&crate::retention::mod_::RetentionRule) -> Result<u64, BlobError>;
}
```

------

## src/policy.rs（占位）

```rust
#[derive(Clone, Debug)]
pub struct BucketPolicy {
  pub default_private: bool,
  pub versioning: bool,
}
impl Default for BucketPolicy {
  fn default()->Self { Self{ default_private: true, versioning: false } }
}
```

------

## src/retry.rs（占位）

```rust
pub async fn with_retry<F, Fut, T, E>(mut f: F, retries:usize) -> Result<T, E>
where F: FnMut() -> Fut, Fut: std::future::Future<Output=Result<T,E>> {
  let mut last = None;
  for _ in 0..=retries {
    match f().await {
      Ok(v) => return Ok(v),
      Err(e) => { last = Some(e); tokio::time::sleep(std::time::Duration::from_millis(50)).await; }
    }
  }
  Err(last.unwrap())
}
```

------

## src/metrics.rs（占位）

```rust
#[derive(Default)]
pub struct BlobStats { pub puts:u64, pub gets:u64 }
impl BlobStats { pub fn inc_put(&mut self){ self.puts+=1 } pub fn inc_get(&mut self){ self.gets+=1 } }
```

------

## src/s3/mod.rs（stub）

```rust
use crate::{errors::BlobError, model::*, r#trait::BlobStore};
use bytes::Bytes;
use futures_core::Stream;

#[derive(Clone)]
pub struct S3BlobStore; // 预留：用 aws-sdk-s3 / reqwest+sigv4 实现

#[async_trait::async_trait]
impl BlobStore for S3BlobStore {
  async fn put(&self, _bucket:&str, _key:&str, _body:Bytes, _opts:PutOpts) -> Result<BlobRef, BlobError> {
    Err(BlobError::provider_unavailable("S3 adapter not implemented in RIS"))
  }
  async fn put_stream<S>(&self, _bucket:&str, _key:&str, _stream:S, _len:Option<u64>, _opts:PutOpts) -> Result<BlobRef, BlobError>
  where S: Stream<Item = Result<Bytes, BlobError>> + Send + Unpin + 'static {
    Err(BlobError::provider_unavailable("S3 adapter not implemented in RIS"))
  }
  async fn get(&self, _bucket:&str, _key:&str, _opts:GetOpts) -> Result<Bytes, BlobError> {
    Err(BlobError::provider_unavailable("S3 adapter not implemented in RIS"))
  }
  async fn head(&self, _bucket:&str, _key:&str) -> Result<BlobMeta, BlobError> {
    Err(BlobError::provider_unavailable("S3 adapter not implemented in RIS"))
  }
  async fn delete(&self, _bucket:&str, _key:&str) -> Result<(), BlobError> {
    Err(BlobError::provider_unavailable("S3 adapter not implemented in RIS"))
  }
  async fn presign_get(&self, _bucket:&str, _key:&str, _opts:PresignGetOpts) -> Result<String, BlobError> {
    Err(BlobError::provider_unavailable("S3 adapter not implemented in RIS"))
  }
  async fn presign_put(&self, _bucket:&str, _key:&str, _opts:PresignPutOpts) -> Result<String, BlobError> {
    Err(BlobError::provider_unavailable("S3 adapter not implemented in RIS"))
  }
  async fn multipart_begin(&self, _bucket:&str, _key:&str, _opts:MultipartPutOpts) -> Result<MultipartInit, BlobError> {
    Err(BlobError::provider_unavailable("S3 adapter not implemented in RIS"))
  }
}
```

------

## src/fs/mod.rs（FS 适配）

```rust
use crate::{errors::BlobError, model::*, key::ensure_key, r#trait::BlobStore};
use bytes::Bytes;
use futures_core::Stream;
use sha2::{Sha256, Digest};
use std::{path::PathBuf, fs, io::Write};
use chrono::Utc;

#[derive(Clone)]
pub struct FsBlobStore {
  pub root: PathBuf,          // 根目录：root/{bucket}/{key}
  pub presign_secret: String, // 用于开发态 presign 的 HMAC
}

impl FsBlobStore {
  pub fn new(root: impl Into<PathBuf>, secret:&str) -> Self {
    Self { root: root.into(), presign_secret: secret.to_string() }
  }
  fn object_path(&self, bucket:&str, key:&str) -> PathBuf { self.root.join(bucket).join(key) }
  fn meta_path(&self, bucket:&str, key:&str) -> PathBuf { self.root.join(bucket).join(format!("{key}.meta.json")) }
  fn ensure_dirs(&self, path:&PathBuf) -> std::io::Result<()> {
    if let Some(p) = path.parent() { fs::create_dir_all(p)?; }
    Ok(())
  }
  fn etag_hex(bytes:&[u8]) -> String {
    let mut hasher = Sha256::new(); hasher.update(bytes);
    format!("{:x}", hasher.finalize())
  }
}

#[async_trait::async_trait]
impl BlobStore for FsBlobStore {
  async fn put(&self, bucket:&str, key:&str, body:Bytes, opts:PutOpts) -> Result<BlobRef, BlobError> {
    ensure_key(bucket, &format!("{}/{key}", bucket)).map_err(BlobError::schema)?; // 仅做关键校验：tenant 由调用侧加在 key 前缀
    ensure_key(key.split('/').next().unwrap_or(""), key).map_err(BlobError::schema)?; // 确保 key 本身以 tenant/ 开头
    let path = self.object_path(bucket, key);
    self.ensure_dirs(&path).map_err(|e| BlobError::provider_unavailable(&format!("mkdirs: {e}")))?;
    // 原子写：tmp + rename
    let tmp = path.with_extension("uploading");
    {
      let mut f = fs::File::create(&tmp).map_err(|e| BlobError::provider_unavailable(&format!("create: {e}")))?;
      f.write_all(&body).map_err(|e| BlobError::provider_unavailable(&format!("write: {e}")))?;
      f.sync_all().ok();
    }
    fs::rename(&tmp, &path).map_err(|e| BlobError::provider_unavailable(&format!("rename: {e}")))?;

    let etag = Self::etag_hex(&body);
    let content_type = opts.content_type.unwrap_or_else(|| "application/octet-stream".into());
    let created_at_ms = Utc::now().timestamp_millis();

    // 写 meta：开发态用于 retention 与幂等留痕
    let meta = serde_json::json!({
      "envelope_id": opts.envelope_id,
      "created_at_ms": created_at_ms,
      "content_type": content_type,
      "size": body.len(),
      "etag": etag,
    });
    let mpath = self.meta_path(bucket, key);
    self.ensure_dirs(&mpath).map_err(|e| BlobError::provider_unavailable(&format!("mkdirs meta: {e}")))?;
    fs::write(&mpath, serde_json::to_vec(&meta).unwrap()).map_err(|e| BlobError::provider_unavailable(&format!("write meta: {e}")))?;

    Ok(BlobRef{
      bucket: bucket.into(), key: key.into(),
      etag, size: body.len() as u64, content_type, created_at_ms
    })
  }

  async fn put_stream<S>(&self, bucket:&str, key:&str, mut stream:S, _len:Option<u64>, opts:PutOpts) -> Result<BlobRef, BlobError>
  where S: Stream<Item = Result<Bytes, BlobError>> + Send + Unpin + 'static {
    let mut buf = Vec::new();
    while let Some(chunk) = futures_util::stream::StreamExt::next(&mut stream).await {
      buf.extend_from_slice(&chunk?);
    }
    self.put(bucket, key, Bytes::from(buf), opts).await
  }

  async fn get(&self, bucket:&str, key:&str, opts:GetOpts) -> Result<Bytes, BlobError> {
    let path = self.object_path(bucket, key);
    let data = fs::read(&path).map_err(|_| BlobError::not_found("object not found"))?;
    if let Some(et) = &opts.if_none_match {
      if &Self::etag_hex(&data) == et { return Err(BlobError::provider_unavailable("not modified (dev stub)")); }
    }
    if let Some((start,end)) = opts.range {
      let end = end.min(data.len() as u64 - 1) as usize;
      let start = start.min(end as u64) as usize;
      return Ok(Bytes::from(data[start..=end].to_vec()));
    }
    Ok(Bytes::from(data))
  }

  async fn head(&self, bucket:&str, key:&str) -> Result<BlobMeta, BlobError> {
    let path = self.object_path(bucket, key);
    let meta_path = self.meta_path(bucket, key);
    let data = fs::read(&path).map_err(|_| BlobError::not_found("object not found"))?;
    let etag = Self::etag_hex(&data);
    let size = data.len() as u64;
    let content_type = fs::read(&meta_path).ok()
      .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
      .and_then(|v| v.get("content_type").and_then(|x| x.as_str()).map(|s| s.to_string()))
      .unwrap_or_else(|| "application/octet-stream".into());
    let created_at_ms = fs::read(&meta_path).ok()
      .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
      .and_then(|v| v.get("created_at_ms").and_then(|x| x.as_i64()))
      .unwrap_or_else(|| Utc::now().timestamp_millis());

    Ok(BlobMeta{
      ref_: BlobRef{ bucket: bucket.into(), key: key.into(), etag, size, content_type, created_at_ms },
      md5_b64: None, user_tags: None, storage_class: None
    })
  }

  async fn delete(&self, bucket:&str, key:&str) -> Result<(), BlobError> {
    let path = self.object_path(bucket, key);
    let m = self.meta_path(bucket, key);
    fs::remove_file(&path).map_err(|_| BlobError::not_found("object not found"))?;
    let _ = fs::remove_file(&m);
    Ok(())
  }

  async fn presign_get(&self, bucket:&str, key:&str, opts:PresignGetOpts) -> Result<String, BlobError> {
    crate::fs::presign::presign_get(&self.presign_secret, bucket, key, opts.expire_secs)
  }

  async fn presign_put(&self, bucket:&str, key:&str, opts:PresignPutOpts) -> Result<String, BlobError> {
    crate::fs::presign::presign_put(&self.presign_secret, bucket, key, opts.expire_secs, opts.content_type.unwrap_or_default(), opts.size_hint)
  }

  async fn multipart_begin(&self, bucket:&str, key:&str, _opts:MultipartPutOpts) -> Result<MultipartInit, BlobError> {
    // 开发态直接给个占位
    let href = self.head(bucket, key).await.unwrap_or(BlobMeta{
      ref_: BlobRef{ bucket: bucket.into(), key: key.into(), etag:String::new(), size:0, content_type:"application/octet-stream".into(), created_at_ms: Utc::now().timestamp_millis() },
      md5_b64: None, user_tags: None, storage_class: None
    }).ref_;
    Ok(MultipartInit{ upload_id: format!("mp-{}", href.key), ref_hint: href })
  }
}
```

------

## src/fs/presign.rs（开发态 Presign）

```rust
use hmac::{Hmac, Mac};
use sha2::Sha256;
use crate::errors::BlobError;

pub fn presign_get(secret:&str, bucket:&str, key:&str, expire_secs:u32) -> Result<String, BlobError> {
  let exp = chrono::Utc::now().timestamp() + expire_secs as i64;
  let to_sign = format!("GET\n{bucket}\n{key}\n{exp}");
  let sig = hmac_sha256(secret, &to_sign);
  let url = format!("fs:///{bucket}/{key}?exp={exp}&sig={}", urlencoding::encode(&sig));
  Ok(url)
}
pub fn presign_put(secret:&str, bucket:&str, key:&str, expire_secs:u32, ct:String, size:Option<u64>) -> Result<String, BlobError> {
  let exp = chrono::Utc::now().timestamp() + expire_secs as i64;
  let to_sign = format!("PUT\n{bucket}\n{key}\n{exp}\n{ct}\n{}", size.unwrap_or(0));
  let sig = hmac_sha256(secret, &to_sign);
  let url = format!("fs:///{bucket}/{key}?exp={exp}&ct={}&size={}&sig={}", urlencoding::encode(&ct), size.unwrap_or(0), urlencoding::encode(&sig));
  Ok(url)
}

/// 测试辅助：校验签名与过期
pub fn verify_url(secret:&str, method:&str, bucket:&str, key:&str, exp:i64, ct:Option<&str>, size:Option<u64>, sig:&str) -> bool {
  if chrono::Utc::now().timestamp() > exp { return false; }
  let base = match method {
    "GET" => format!("GET\n{bucket}\n{key}\n{exp}"),
    "PUT" => format!("PUT\n{bucket}\n{key}\n{exp}\n{}\n{}", ct.unwrap_or(""), size.unwrap_or(0)),
    _ => return false
  };
  hmac_sha256(secret, &base) == sig
}

fn hmac_sha256(secret:&str, msg:&str) -> String {
  let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
  mac.update(msg.as_bytes());
  let out = mac.finalize().into_bytes();
  base64::engine::general_purpose::STANDARD.encode(out)
}
```

------

## src/retention/mod.rs（简单 Retention 执行器）

```rust
use crate::{errors::BlobError};
use std::path::{PathBuf, Path};
use std::fs;
use chrono::{Utc, Duration};

#[derive(Clone, Debug)]
pub struct RetentionRule {
  pub bucket:String,
  pub class:RetentionClass,
  pub selector:Selector,   // tenant & optional namespace
  pub ttl_days:u32,
  pub archive_to:Option<String>,
  pub version_hash:String,
}
#[derive(Clone, Debug)]
pub enum RetentionClass { Hot, Warm, Cold, Frozen }
#[derive(Clone, Debug)]
pub struct Selector { pub tenant:String, pub namespace:Option<String>, pub tags:std::collections::BTreeMap<String,String> }

#[derive(Clone)]
pub struct FsRetentionExec { pub root: PathBuf }
impl FsRetentionExec { pub fn new(root: impl Into<PathBuf>) -> Self { Self{ root: root.into() } } }

#[async_trait::async_trait]
impl crate::r#trait::RetentionExec for FsRetentionExec {
  async fn apply_rule(&self, rule:&crate::retention::mod_::RetentionRule) -> Result<u64, BlobError> {
    let bucket_dir = self.root.join(&rule.bucket);
    if !bucket_dir.exists() { return Ok(0); }

    let mut count = 0u64;
    let base_prefix = if let Some(ns) = &rule.selector.namespace {
      format!("{}/{}", rule.selector.tenant, ns)
    } else { rule.selector.tenant.clone() };

    let cutoff = Utc::now() - Duration::days(rule.ttl_days as i64);
    // 简化：按文件 mtime 判断是否过期
    fn visit(dir:&Path, files:&mut Vec<PathBuf>) {
      if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
          let p = e.path();
          if p.is_dir() { visit(&p, files); } else if p.extension().and_then(|x| x.to_str()) != Some("json") { files.push(p); }
        }
      }
    }
    let mut files = vec![]; visit(&bucket_dir, &mut files);
    for f in files {
      if let Some(rel) = f.strip_prefix(&bucket_dir).ok().and_then(|p| p.to_str()) {
        let rel = rel.replace('\\', "/");
        if !rel.starts_with(&base_prefix) { continue; }
        let meta = fs::metadata(&f).map_err(|e| BlobError::provider_unavailable(&format!("stat: {e}")))?;
        let mtime = meta.modified().ok().and_then(|t| t.elapsed().ok()).map(|el| Utc::now() - chrono::Duration::from_std(el).unwrap()).unwrap_or(Utc::now());
        // 若 ttl_days==0，立即生效
        if rule.ttl_days == 0 || mtime < cutoff {
          fs::remove_file(&f).ok();
          count += 1;
        }
      }
    }
    Ok(count)
  }
}
```

------

## src/prelude.rs

```rust
pub use crate::errors::BlobError;
pub use crate::model::*;
pub use crate::r#trait::{BlobStore, RetentionExec};
pub use crate::fs::mod_::FsBlobStore;
pub use crate::retention::mod_::{FsRetentionExec, RetentionRule, RetentionClass, Selector};
```

------

## tests/e2e.rs

```rust
use soulbase_blob::prelude::*;
use tempfile::tempdir;
use bytes::Bytes;

#[tokio::test]
async fn put_get_delete_roundtrip_fs() {
    let tmp = tempdir().unwrap();
    let store = FsBlobStore::new(tmp.path(), "dev-secret");

    let bucket = "artifacts";
    let key = "tenantA/reports/202501/01/u1-report.json";
    // put
    let br = store.put(bucket, key, Bytes::from_static(b"{\"ok\":true}"), PutOpts{ content_type: Some("application/json".into()), ..Default::default() }).await.unwrap();
    assert_eq!(br.bucket, bucket);
    assert_eq!(br.key, key);
    assert!(br.size > 0);

    // head
    let meta = store.head(bucket, key).await.unwrap();
    assert_eq!(meta.ref_.etag, br.etag);
    assert_eq!(meta.ref_.content_type, "application/json");

    // get
    let got = store.get(bucket, key, GetOpts::default()).await.unwrap();
    assert_eq!(&got[..], b"{\"ok\":true}");

    // delete
    store.delete(bucket, key).await.unwrap();
    let err = store.get(bucket, key, GetOpts::default()).await.err().unwrap();
    let msg = format!("{}", err);
    assert!(msg.contains("Object not found"));
}

#[tokio::test]
async fn presign_get_expires() {
    let tmp = tempdir().unwrap();
    let store = FsBlobStore::new(tmp.path(), "dev-secret");

    let url = store.presign_get("b", "tenantB/data/202501/01/x.bin", PresignGetOpts{ expire_secs: 1 }).await.unwrap();
    // 解析
    let u = url::Url::parse(&url.replace("fs:","http:")).unwrap(); // 只为解析 query
    let exp: i64 = u.query_pairs().find(|(k,_)| k=="exp").map(|(_,v)| v.parse().unwrap()).unwrap();
    let sig = u.query_pairs().find(|(k,_)| k=="sig").map(|(_,v)| v.to_string()).unwrap();
    let ok_now = soulbase_blob::fs::presign::verify_url("dev-secret", "GET", "b", "tenantB/data/202501/01/x.bin", exp, None, None, &sig);
    assert!(ok_now, "should be valid before expiry");

    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let ok_late = soulbase_blob::fs::presign::verify_url("dev-secret", "GET", "b", "tenantB/data/202501/01/x.bin", exp, None, None, &sig);
    assert!(!ok_late, "should be invalid after expiry");
}

#[tokio::test]
async fn retention_rule_immediate() {
    let tmp = tempdir().unwrap();
    let store = FsBlobStore::new(tmp.path(), "dev-secret");

    // put 两个对象
    let bucket = "evidence";
    let k1 = "tenantZ/screens/202501/01/a.png";
    let k2 = "tenantZ/screens/202501/01/b.png";
    let _ = store.put(bucket, k1, Bytes::from_static(b"1111"), PutOpts::default()).await.unwrap();
    let _ = store.put(bucket, k2, Bytes::from_static(b"2222"), PutOpts::default()).await.unwrap();

    // 立即生效的 Retention（ttl_days=0）
    let exec = FsRetentionExec::new(tmp.path());
    let rule = RetentionRule {
        bucket: bucket.into(),
        class: RetentionClass::Cold,
        selector: Selector{ tenant:"tenantZ".into(), namespace: Some("screens".into()), tags: Default::default() },
        ttl_days: 0,
        archive_to: None,
        version_hash: "v1".into(),
    };
    let n = exec.apply_rule(&rule).await.unwrap();
    assert!(n >= 2);

    // 已删除
    assert!(store.get(bucket, k1, GetOpts::default()).await.is_err());
    assert!(store.get(bucket, k2, GetOpts::default()).await.is_err());
}
```

------

## README.md（简要）

```markdown
# soulbase-blob (RIS)

统一对象/制品存储：
- 抽象：BlobStore / RetentionExec
- 模型：BlobRef / BlobMeta / Digest / Put/Get/Presign / Multipart 占位
- 开发态 FS 适配（原子写、ETag=sha256、Presign=HMAC）
- S3/MinIO 适配 stub
- Retention 执行器（FS）按 {tenant}/{namespace}/ 前缀 + TTL 删除

## 快速开始
let store = FsBlobStore::new("/var/lib/soul/blob", "dev-secret");
let r = store.put("artifacts", "tenantA/reports/202501/01/u1.json", bytes::Bytes::from_static(b"{}"), PutOpts::default()).await?;
let url = store.presign_get("artifacts", &r.key, PresignGetOpts{ expire_secs: 300 })?;

## 测试
cargo test
```

------

### 后续接入指引

- **S3/MinIO**：在 `s3/mod.rs` 实现 `put/get/head/delete/presign/multipart_*`（推荐 `aws-sdk-s3`）；
- **安全**：在 `FsBlobStore` 与 S3 实现里补充 `tenant` 前缀强校验（当前已做基础校验），Presign 统一加入 IP/CT/大小限制；
- **观测/成本**：若启 `observe/qos` feature，对 Put/Get/Delete/MultiPart/Retention 打点并计入账页；
- **与 Sandbox/Tools 集成**：将产物与证据只保存 `BlobRef + Digest`，避免日志落原文内容。
