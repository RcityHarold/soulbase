# 拦截器职责切分（Issue 06 对齐）

> 在不改变原文结构的前提下，明确 SB‑19 与 SB‑05 的职责边界：复用 Auth/Audit，Net 仅保留网络侧能力；并强调“仅幂等方法自动重试”。

## 职责边界

- 复用（不自带实现）：
  - AuthN/AuthZ/Consent/Quota 审计（由 SB‑05 提供拦截面，SB‑04 提供鉴权门面）；
  - 公共错误视图映射（SB‑02）。
- Net 保留：
  - 超时（connect/ttfb/total/read/write）与分相位指标；
  - 断路器/重试（遵循 Retry‑After、指数退避+抖动）；
  - 安全（私网/链路本地/环回拦截、TLS 校验）；
  - 限并发/速率；
  - `QoS‑Bytes` 统计；
  - `CacheHook/SWR` 只读路径缓存；
  - `Trace/UA/EnvId` 注入（`X-Env-Id`）。

## 幂等重试

- 仅对幂等方法（GET/HEAD/OPTIONS/部分 PUT/DELETE）或显式标注幂等的 POST 启用重试；
- 与 Issue 02 对齐：端到端的幂等主锚为 `Envelope.envelope_id`；
- 对 5xx/429/DNS/连接类错误按策略重试；遵守 `Retry‑After`。

## 集成方式（建议）

- Net 的 Bearer/JWS 等鉴权拦截器使用 SB‑04/05 提供的实现或适配，不在 SB‑19 复制逻辑；
- 统一从 SB‑05 获取 `Trace/Subject/Tenant/Consent` 等上下文，并在 Net 仅做“注入与透传”。

## 验收

- 代码层面（RIS/示例）无重复的 Auth/Audit 实现；
- 幂等方法才自动重试；
- 指标可见分相位时延、重试次数、断路器状态，且错误码统一映射到 SB‑02 公共视图。
