# Net 对接（Issue 06 对齐）

> 明确拦截面与 Net 的职责分工与对接口径，避免重复实现。

## 职责分工

- SB‑05：集中实现 AuthN/AuthZ/Consent/Quota 审计、公共错误视图、响应头 `X-Config-*`、`Trace/Subject/Tenant/Consent` 注入；
- SB‑19：仅负责网络层超时/重试/断路器/安全/限流/`QoS‑Bytes`/`CacheHook` 与 `Trace/UA/EnvId` 注入。

## 对接口径

- Net 复用 SB‑05 提供的 Auth/Audit 拦截器或适配器，不重复实现；
- 错误统一调用 SB‑02 公共视图；
- 幂等重试仅对幂等方法或显式幂等 POST 生效。

## 验收

- 文档/RIS 中无“双处实现 Auth/Audit”的重复；
- 出站流量具备 `Trace/UA/EnvId`；
- 幂等方法才会被重试，遵守 `Retry‑After`。
