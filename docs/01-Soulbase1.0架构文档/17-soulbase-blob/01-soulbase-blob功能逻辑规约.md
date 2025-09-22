### **文档 SB-17：soulbase-blob（对象/制品存储 · 统一适配 / Objects & Artifacts Store）**

#### **第一部分：功能逻辑规约**

------

#### **0. 目标与范围（Objectives & Scope）**

- **目标**：提供一个**统一、可观测、可治理**的**对象/制品存储层**，面向 Sandbox/Tools 的执行产物、Evidence 附件、日志/报告归档、A2A 对账附件、模型/配置快照等非结构化数据，做到：
  1. **标准化对象接口**：Put/Get/Delete/Head/Presign（签名 URL），支持**多后端**（S3/MinIO/本地 FS/自建）。
  2. **最小披露与隔离**：强制**租户隔离**、默认私有、Key 命名规范、可选透明加密与加盐摘要。
  3. **生命周期治理**：按 `RetentionRule`（Hot/Warm/Cold/Frozen）做 TTL、分级归档、搬迁与删改留痕。
  4. **可观测与成本**：统一**字节/对象数/请求时延**指标，配合 QoS 记账，支持**并发/速率**控制与重试。
  5. **大对象友好**：分片上传/断点续传、流式读写、内容散列（ETag）与幂等写。
- **范围**：
  - 抽象：`BlobStore`（Put/Get/Del/Presign）、`Multipart`（分片）、`Retention`（生命周期），`BlobRef/Meta/Digest`（元数据）。
  - 策略：Key 规范、租户/命名空间、默认加密/ACL、默认私有、跨区域复制（CRR）可选。
  - 与 `soulbase-observe`（指标）、`soulbase-qos`（账页/留存）、`soulbase-config`（后端与桶策略）、`soulbase-sandbox/tools/tx/a2a/benchmark/contract-testkit` 的对接位。
- **非目标**：不提供通用 CDN；不做在线内容转换（如图片缩放/转码）；不承诺强事务（交由 `soulbase-tx` 与业务实现幂等）。

------

#### **1. 功能定位（Functional Positioning）**

- **平台级对象/制品中台**：统一对象写读、签名分发、留存归档与审计证据，避免“各模块各接 S3/FS”。
- **安全与合规“落点”**：以**默认私有、租户前缀、签名 URL**为基准；配合 `RetentionRule` 执行**数据生命周期**与**删除留痕**。
- **成本与性能**：集中出入口以便 QoS 计费与 Observe 观测，支持**分片大文件**与**SWR 型只读缓存**（通过 SB-16 cache）。

------

#### **2. 系统角色与地位（System Role & Position）**

- 层级：**基石层 / Soul-Base**（横切多模块）。
- 关系：
  - **SB-06 Sandbox / SB-08 Tools**：执行产物（截图、抓取内容、导出包）写入 Blob，Evidence 中仅存 `BlobRef` 与摘要。
  - **SB-11 Observe**：基准/契约报告、运行日志归档（只存摘要/链接）；暴露 `blob_*` 指标。
  - **SB-14 QoS**：对象/字节写读计量入账；`RetentionRule` 与 QoS 的留存/归档策略对齐。
  - **SB-15 A2A**：对账附件、收据链快照上传，跨域分发用 `presign_get`。
  - **SB-03 Config**：后端选择、桶策略（SSE/KMS、版本、CRR）按快照生效。

------

#### **3. 核心概念与数据模型（Core Concepts & Data Model）**

- **Bucket/Namespace**：逻辑桶；按功能/保密等级划分，如 `evidence`, `artifacts`, `reports`, `a2a`。
- **Key 命名规范**：`{tenant}/{namespace}/{yyyymm}/{dd}/{ulid}-{suffix}`
  - 强制 `tenant` 作为首段；允许追加 `project/subject` 次级隔离；禁止用户自带 `..` 等路径转义。
- **BlobRef**：`{bucket, key, etag, size, content_type, created_at}`（只读摘要）。
- **Digest/Commitment**：`algo + b64 + size`，用于 Evidence 与 A2A 承诺。
- **ACL/加密**：默认私有；支持 SSE-S3/KMS；Presign 仅用于短时公开读/写。
- **Multipart**：大对象分片（阈值如 16–64MB）；分片 ETag/合并 ETag；可断点续传。

------

#### **4. 不变式（Invariants）**

1. **默认私有**：任何 Put 不显式设置 ACL 时不可公开读取；Presign 需**显式**时效与 IP/路径限制（如支持）。
2. **租户隔离**：Key 必含 `tenant` 前缀；跨租户读写拒绝；CRR 需带租户映射规则。
3. **最小披露**：Evidence 中只存 `BlobRef`/`Digest`/`Presign`，不嵌入原文内容；日志不落对象原文。
4. **等幂写**：同一 `envelope_id`/`key` 的 Put 返回相同 `BlobRef`（依 ETag）；避免重复写入计费。
5. **可观测**：每次 Put/Get/Delete/Head 产出 `bytes/latency` 指标；错误走稳定码。
6. **留存可执行**：有 `RetentionRule` 的对象必须在 TTL 到期后**归档/删除**并写 Evidence（删除留痕）。

------

#### **5. 能力与接口（Abilities & Abstract Interfaces）**

> 详设在 TD/RIS；此处为行为口径。

- **Put/Upload**：
  - `put(bucket, key, bytes, content_type, opts{ttl_days?, encrypt?, tags?}) -> BlobRef`
  - `multipart.begin -> upload_part(i, bytes) -> complete -> BlobRef`
  - **幂等**：同 key + 内容相同 → 返回已有 `BlobRef`（通过 ETag 比对或业务幂等锚）。
- **Get/Stream/Head**：
  - `get(bucket, key) -> stream/bytes`，`head(bucket, key) -> meta`；默认仅**私有**访问；
  - `range`/`If-None-Match` 支持（提升缓存命中）。
- **Delete**：
  - `delete(bucket, key)`；若受 `RetentionRule` 约束，则**软删除/归档**并记录 Evidence。
- **Presign**：
  - `presign_get(bucket, key, expire_secs) -> url`
  - `presign_put(bucket, key, expire_secs, content_type, size?) -> url`
  - 附带**条件**（方法/CT/大小/MD5）与**最小权限**。
- **Retention/Lifecycle**：
  - `apply_rule(rule)`：按 Selector（bucket/label/tenant）批量迁移 Hot→Warm→Cold/Frozen；
  - `expunge`：超保留期数据硬删除（输出 Evidence）。
- **治理/扫描**：
  - 按前缀/标签列举；**清单快照**（Manifest）导出；
  - 并发/速率限制、重试策略（幂等 PUT 重试安全）。

------

#### **6. 指标与 SLO / 验收（Metrics, SLOs & Acceptance）**

- **指标**：
  - `blob_put_total{bucket}`、`blob_get_total{bucket}`、`blob_delete_total{bucket}`
  - `blob_bytes{dir=in|out,bucket}`、`blob_latency_ms_bucket{op=put|get|head|delete}`
  - `blob_presign_total{op}`、`blob_multipart_total{phase}`、`blob_retention_archived_total{class}`
- **SLO**：
  - Put/Get p95：本地/同机房**≤ 30ms**（小对象）、跨区与大对象不承诺；
  - 失败重试后成功率 ≥ **99.9%**；
  - Retention 作业日成功率 ≥ **99.9%**；删除留痕覆盖率 100%。
- **验收**：
  - 契约：租户前缀校验/ACL 默认私有/Presign 失效/幂等写/删除留痕；
  - 基准：多并发下 Put/Get 尾延稳定；
  - 混沌：S3/MinIO 问题时自动退避重试/限速；不影响主服务稳定性。

------

#### **7. 依赖与边界（Dependencies & Boundaries）**

- **上游**：SB-06/08/10/11/12/13/15；
- **下游**：S3/MinIO/FS 等对象后端；
- **边界**：不直接暴露公网读写（走 Presign）；不负责 CDN；不提供强事务多对象操作（必要时交由 SB-10 Saga/Outbox）。

------

#### **8. 风险与控制（Risks & Controls）**

- **敏感数据泄露**：默认私有 + Presign 限时；Key/Ref 不含敏感语义；Evidence 不嵌原文。
- **租户串读**：Key 强制 `tenant` 前缀 + 服务器侧 ACL 校验；Presign 校验 bucket/key 与租户映射。
- **写放大与重复计费**：启用 PUT 幂等策略（ETag/If-Match/`envelope_id`）；分片合并前清理过期分片。
- **成本激增**：QoS 监控 `bytes_out/in` 与请求量，拉闸策略（限速/分片大小）。
- **生命周期失效**：Retention 失败进入死信/重试队列，审计 Evidence 必须写入。

------

#### **9. 关键交互序列（Key Interaction Sequences）**

**9.1 Sandbox 产物写入**

1. Sandbox 执行完成→计算 `Digest` → 调用 `put()`；
2. 返回 `BlobRef` 写入 Evidence；
3. Outbound 对外分发时，若需要临时给前端/对端读取，调用 `presign_get()`。

**9.2 契约/基准报告归档**

1. 生成报告文件 → `put(reports, tenant/…/ulid.json)`；
2. Evidence/CI 注入 `BlobRef` 链接与摘要；
3. Retention：`Frozen` 90 天后自动归档或删除。

**9.3 A2A 对账附件**

1. 本地生成账页 CSV → `put(a2a, tenant/…/period.csv)`；
2. 通过 A2A 发送 `BlobRef` + `Digest`；
3. 对端使用 `presign_get()` 或 A2A 代理下载。

------

#### **10. 开放问题（Open Issues / TODO）**

- **透明加密策略**：默认 SSE-S3/KMS 还是应用层加密（AEAD）；密钥轮换与兼容。
- **跨区域复制（CRR）**：是否纳入统一策略（灾备/低时延读取）。
- **上传协议统一**：前端直传（浏览器）是否支持表单直传/分片直传的安全前置校验。
- **并发与限速**：是否暴露租户级写入速率器，与 SB-14 协同。
- **目录清单快照**：对审计/合规是否需要周期导出“对象清单”并签名存档。

------

> 若你认可本规约，下一步我将输出 **SB-17-TD（技术设计）**：定义 `BlobStore/Multipart/Retention` trait、`BlobRef/Meta/Digest` DTO、Key 规范与签名 URL 细节、S3/FS 适配接口、重试/幂等/观察的实现要点；随后提供 **SB-17-RIS** 可运行骨架（本地 FS + S3/MinIO stub + 2–3 个端到端单测）。
