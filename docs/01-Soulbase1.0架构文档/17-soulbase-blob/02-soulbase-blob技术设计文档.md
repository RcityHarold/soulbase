# 文档 SB-17-TD：`soulbase-blob` 技术设计

（Objects & Artifacts Store · S3/MinIO/FS 统一适配 · Presign · Multipart · Retention）

> 对应规约：SB-17
>  目标：给出**可落地**的 Rust 设计：`BlobStore/Multipart/Retention` trait，`BlobRef/Meta/Digest` DTO，Key 命名与幂等策略、Presign 与条件请求、S3/MinIO/本地 FS 适配接口，重试/退避与观测、与 `SB-11 observe`/`SB-14 qos`/`SB-06 sandbox`/`SB-08 tools`/`SB-15 a2a` 对接点。
>  说明：以下为接口与实现要点（RIS 在下一步给出）。

------

## 1. Crate 结构与 Feature 规划

```
soulbase-blob/
  src/
    lib.rs
    errors.rs            # BlobError → SB-02 错误映射（PROVIDER.UNAVAILABLE / SCHEMA.VALIDATION_FAILED …）
    model.rs             # BlobRef / BlobMeta / Digest / PutOpts / GetOpts / PresignOpts / Multipart*
    key.rs               # Key 规范：{tenant}/{namespace}/{yyyymm}/{dd}/{ulid}-{suffix}
    trait.rs             # BlobStore / Multipart / Retention trait
    policy.rs            # BucketPolicy / Encryption / ACL / CRR（来自 SB-03 快照）
    retry.rs             # Backoff/Retry 策略（幂等 PUT/HEAD/GET）
    metrics.rs           # SB-11 指标钩子（blob_*）
    s3/
      mod.rs             # S3/MinIO 适配（reqwest/aws-sigv4 或 rusoto/awssdk）
      signer.rs          # v4 签名或直传表单策略
      presign.rs         # 预签名 URL 实现
    fs/
      mod.rs             # 本地开发 FS 适配（root dir / file IO）
      presign.rs         # 开发态伪 presign（受服务端代理校验）
    retention/
      mod.rs             # RetentionExec（Selector→归档/删除）
    prelude.rs
```

**features**

- `backend-s3`（默认）/`backend-fs`（开发态）
- `kms`（SSE-KMS 配置）
- `observe`（对接 SB-11）
- `qos`（字节/对象成本打点给 SB-14）

------

## 2. 数据模型（`model.rs`）

```rust
use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlobRef {
  pub bucket: String,
  pub key: String,              // {tenant}/{namespace}/YYYYMM/DD/{ulid}-{suffix}
  pub etag: String,             // 后端 ETag（S3: 引号内十六进制；FS: sha256）
  pub size: u64,
  pub content_type: String,
  pub created_at_ms: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlobMeta {
  pub ref_: BlobRef,
  pub md5_b64: Option<String>,  // 校验可选
  pub user_tags: Option<std::collections::BTreeMap<String,String>>,
  pub storage_class: Option<String>, // STANDARD/GLACIER…
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Digest { pub algo: &'static str, pub b64: String, pub size: u64 } // Evidence / A2A 承诺

#[derive(Clone, Debug, Default)]
pub struct PutOpts {
  pub content_type: Option<String>,
  pub ttl_days: Option<u32>,            // 用于 Retention 策略提示
  pub encrypt: bool,                    // SSE-S3/KMS（由 policy 决定）
  pub user_tags: Option<std::collections::BTreeMap<String,String>>,
  pub envelope_id: Option<String>,      // 幂等锚（SB-10/14/15）
}

#[derive(Clone, Debug, Default)]
pub struct GetOpts {
  pub range: Option<(u64, u64)>,        // [start, end] 包含式
  pub if_none_match: Option<String>,    // ETag 条件
}

#[derive(Clone, Debug, Default)]
pub struct PresignGetOpts { pub expire_secs: u32 }

#[derive(Clone, Debug, Default)]
pub struct PresignPutOpts {
  pub expire_secs: u32,
  pub content_type: Option<String>,
  pub size_hint: Option<u64>,           // 限制大小
}
```

### Multipart（大对象）

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MultipartInit { pub upload_id: String, pub ref_hint: BlobRef }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PartETag { pub part_number: u32, pub etag: String, pub size: u64 }

#[derive(Clone, Debug)]
pub struct MultipartPutOpts { pub content_type: Option<String>, pub encrypt: bool, pub envelope_id: Option<String> }
```

------

## 3. 键名与幂等（`key.rs`）

- **规范**：`{tenant}/{namespace}/{yyyymm}/{dd}/{ulid}-{suffix}`；`suffix` 由业务决定（`.json/.png/.csv`）。
- **幂等**：`PutOpts.envelope_id` 存入对象元数据（`x-soul-envelope-id`）；再次 PUT 同 key 若 `If-None-Match`/`If-Match` 与 ETag 一致→返回已有 `BlobRef`。
- **校验**：拒绝 `..`、重复斜杠、控制字符；长度上限（推荐 1k）。

------

## 4. 抽象 Trait（`trait.rs`）

```rust
use crate::{errors::BlobError, model::*, policy::BucketPolicy};
use bytes::Bytes;
use futures_core::Stream;

#[async_trait::async_trait]
pub trait BlobStore: Send + Sync {
  async fn put(&self, bucket:&str, key:&str, body:Bytes, opts:PutOpts) -> Result<BlobRef, BlobError>;
  async fn put_stream<S>(&self, bucket:&str, key:&str, stream:S, content_len:Option<u64>, opts:PutOpts) -> Result<BlobRef, BlobError>
  where S: Stream<Item = Result<Bytes, BlobError>> + Send + Unpin + 'static;

  async fn get(&self, bucket:&str, key:&str, opts:GetOpts) -> Result<Bytes, BlobError>; // RIS：一次性；生产可返回 Stream
  async fn head(&self, bucket:&str, key:&str) -> Result<BlobMeta, BlobError>;
  async fn delete(&self, bucket:&str, key:&str) -> Result<(), BlobError>;

  async fn presign_get(&self, bucket:&str, key:&str, opts:PresignGetOpts) -> Result<String, BlobError>;
  async fn presign_put(&self, bucket:&str, key:&str, opts:PresignPutOpts) -> Result<String, BlobError>;

  async fn multipart_begin(&self, bucket:&str, key:&str, opts:MultipartPutOpts) -> Result<MultipartInit, BlobError>;
  async fn multipart_put_part(&self, bucket:&str, key:&str, upload_id:&str, part_number:u32, bytes:Bytes) -> Result<PartETag, BlobError>;
  async fn multipart_complete(&self, bucket:&str, key:&str, upload_id:&str, parts:Vec<PartETag>) -> Result<BlobRef, BlobError>;
  async fn multipart_abort(&self, bucket:&str, key:&str, upload_id:&str) -> Result<(), BlobError>;

  fn bucket_policy(&self, bucket:&str) -> Option<BucketPolicy>; // 供上层参考
}

#[async_trait::async_trait]
pub trait RetentionExec: Send + Sync {
  async fn apply_rule(&self, rule:&RetentionRule) -> Result<u64, BlobError>; // 返回处理对象数
}

#[derive(Clone, Debug)]
pub struct RetentionRule {
  pub bucket: String,                          // 作用桶
  pub class: RetentionClass,                   // Hot/Warm/Cold/Frozen
  pub selector: Selector,                      // labels/tenant 前缀匹配
  pub ttl_days: u32,
  pub archive_to: Option<String>,              // 目的地（跨 bucket/class）
  pub version_hash: String,
}

#[derive(Clone, Debug)]
pub enum RetentionClass { Hot, Warm, Cold, Frozen }
#[derive(Clone, Debug)]
pub struct Selector { pub tenant: String, pub namespace: Option<String>, pub tags: std::collections::BTreeMap<String,String> }
```

------

## 5. 策略与配置（`policy.rs`）

```rust
#[derive(Clone, Debug)]
pub struct BucketPolicy {
  pub default_private: bool,         // 默认私有
  pub encryption: Encryption,        // None | SSE-S3 | SSE-KMS{key_id}
  pub versioning: bool,              // 版本化（S3）
  pub allowed_content_types: Option<Vec<String>>,
  pub crr: Option<CrrPolicy>,        // Cross-Region Replication
  pub upload_part_min_bytes: u64,    // 分片阈值（如 8MB/16MB）
}

#[derive(Clone, Debug)]
pub enum Encryption { None, SseS3, SseKms{ key_id: String } }

#[derive(Clone, Debug)]
pub struct CrrPolicy { pub enabled: bool, pub target_bucket: String, pub async_copy: bool }
```

> 配置来源：`soulbase-config` 快照；`BlobStore` 初始化时加载并以**快照哈希**固化（证据/账页使用）。

------

## 6. 重试与退避（`retry.rs`）

- **读（GET/HEAD）**：连接类错误/5xx → 指数退避 + 抖动；重试 2–3 次；
- **写（PUT/分片）**：仅幂等条件下重试：
  - 普通 PUT：使用 `If-Match/If-None-Match` 控制；
  - Multipart：对**单个 part** 重试安全（S3 以 part_number 幂等），`complete` 前可多次 `put_part`；
- **Presign**：失败不重试（由客户端持有 URL 并重试）。

------

## 7. 指标与观测（`metrics.rs`）

- `blob_put_total{bucket}` / `blob_get_total{bucket}` / `blob_delete_total{bucket}`
- `blob_bytes{dir=in|out,bucket}`
- `blob_latency_ms_bucket{op}`：put|get|head|delete（5/10/20/50/100/200/500/1000ms）
- `blob_multipart_total{phase=begin|part|complete|abort}`
- `blob_retention_archived_total{class}`
- 标签：`tenant,bucket`（从 key 首段解析）；**严禁**记录对象内容/完整 key（只展示前缀/hash）。

------

## 8. 错误映射（`errors.rs`）

| 场景                                      | 稳定码                                                       |
| ----------------------------------------- | ------------------------------------------------------------ |
| 连接/超时/服务不可用                      | `PROVIDER.UNAVAILABLE`                                       |
| 认证/权限（S3 403/签名错）                | `AUTH.FORBIDDEN`                                             |
| 请求参数/条件冲突（If-None-Match 失败等） | `SCHEMA.VALIDATION_FAILED` 或 `STORAGE.CONFLICT`             |
| 对象不存在                                | `STORAGE.NOT_FOUND`                                          |
| 多分片合并失败                            | `UNKNOWN.INTERNAL`（或专用 `BLOB.MULTIPART_FAILED`，如在 SB-02 新增） |

> 对外只返回**公共视图**：`{code, message, correlation_id}`；完整错误写 Evidence。

------

## 9. S3/MinIO 适配（`s3/*`）

- **依赖**：可选 `aws-sdk-s3` 或 `reqwest + aws-sigv4`；RIS 先用最少依赖。
- **Put**：`PutObject`（或 `CreateMultipartUpload + UploadPart + Complete`）；`x-amz-meta-…` 写入 `envelope_id` 与 `tenant`；
- **Get/Head**：支持 `Range` 与 `If-None-Match`；
- **Presign**：
  - GET：`GET /{bucket}/{key}?X-Amz-Algorithm=…&X-Amz-Credential=…&X-Amz-Expires=…&X-Amz-Signature=…`
  - PUT：限制 `content-type` 与 `content-length-range`；
- **退避**：429/5xx 基于 `retry.rs`；
- **CRR**：由桶策略控制；SDK 级别不主动触发复制，只读策略。

------

## 10. 本地 FS 适配（`fs/*`）

- 根目录：`root/{bucket}/{key}`；
- Put：原子写（临时文件 + rename）；ETag = sha256；
- Presign（开发态）：生成一个短时 HMAC token，配合**本地代理**校验（不建议直接暴露文件系统 URL）。
- 不支持 Multipart（RIS 可拼接）；产线仅用于本地开发/单测。

------

## 11. Retention 执行（`retention/*`）

- **扫描策略**：
  - S3：`ListObjectsV2` 按前缀 + 标签过滤；
  - FS：目录遍历；
- **动作**：
  - Hot→Warm→Cold：更改 `storage class` 或移动至归档桶；
  - 过期删除：硬删除 + Evidence（`RetentionEvent{count,bucket,class,selector}`）；
- **速率/并发**：`Semaphore` 控并发；字节统计打到 `blob_retention_*` 指标。

------

## 12. 与周边模块对接

- **SB-06/SB-08**：工具产物上传：
  - 先计算 `Digest`（`sha256`）；`put` 返回 `BlobRef`；Evidence 只存 `BlobRef + Digest`；
  - 大文件走 `multipart_*`；前端直传用 `presign_put`（校验 CT/size/tenancy）。
- **SB-11**：所有操作打点；删除/归档写 Evidence；**不要**把对象内容写日志。
- **SB-14**：
  - `blob_bytes{dir}` 纳入成本；周期汇总到 Ledger（按 bucket/tenant）；
  - `RetentionRule` 由 QoS/Config 提供，`RetentionExec` 执行。
- **SB-15**：A2A 对账附件/收据链快照：用 `presign_get` 暴露短时下载；在 A2A 消息里仅传 `BlobRef + Digest`。

------

## 13. 并发与安全

- **限流**：每租户/每 bucket PUT 并发上限；大对象分片并发上限；
- **内存控制**：`put_stream` 边读边传；避免把大对象完全 load 至内存；
- **加密**：默认 SSE-S3 或 KMS（桶策略）；应用层加密作为可选（`soulbase-crypto` 提供 AEAD）；
- **客体隔离**：Key 前缀强校验（必须 `{tenant}/…`）；Presign 校验 `bucket/key` 与租户映射关系；
- **CORS/直传**：如需浏览器直传，Presign 与网关严格限制方法/CT/大小/Headers。

------

## 14. 测试与验收

- **契约测试（SB-13）**：
  - Put 幂等、Key 校验、默认私有、Presign 过期、Range/条件请求；
  - Retention：到期归档/删除与 Evidence。
- **基准（SB-12）**：
  - 小对象 Put/Get p95；并发 64–256 的尾延；
  - Multipart 64MB×N 分片合并时间。
- **混沌**：
  - S3 断开/429/5xx：指数退避 + 限流动作；
  - FS 权限错误/磁盘满：错误码映射正确、业务不崩。

------

## 15. RIS 要点（下一步实现）

- `BlobStore` 两后端：`backend-fs`（本地开发）+ `backend-s3`（生产）
- `put/get/head/delete/presign*` 基本打通；
- Multipart：S3 完整、FS 简化；
- Retention：基于 selector 简单扫描与删除（FS）/标记（S3）；
- 单测：
  1. Put→Head→Get→Delete 正常链；
  2. Presign URL 过期不可用；
  3. Retention：TTL 到期能归档/删除并产出 Evidence（可由 SB-11 的 Memory sink 代替）。

------

### 小结

该 TD 使 `soulbase-blob` 成为**统一、可治理**的对象/制品中台：

- **一致接口**：S3/MinIO/FS 适配透明；
- **安全默认**：私有 + 租户前缀 + Presign 最小披露；
- **治理闭环**：Retention/指标/账页/证据；
- **大对象友好**：multipart、流式、幂等。

确认无误后，我将输出 **SB-17-RIS（最小可运行骨架）**：FS 适配 + S3 stub、`BlobStore` 基本功能、Presign（开发态）、Retention 执行器，以及 2–3 个端到端单测（上传→读取→删除 / presign 过期 / retention 归档）。
