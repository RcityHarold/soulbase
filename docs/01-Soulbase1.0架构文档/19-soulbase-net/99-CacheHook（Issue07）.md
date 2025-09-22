# CacheHook（Issue 07 对齐）

> 明确 Net 在只读路径上的缓存钩子与 SWR 行为，避免对写路径产生副作用。

## 只读路径缓存

- 仅对 GET/HEAD 开启 `CacheHook/SWR`；
- 利用 `ETag/Last-Modified` 与 `If-None-Match/If-Modified-Since` 做条件请求；
- 命中 304 时刷新本地缓存的 TTL；
- 不在 POST/PUT/PATCH/DELETE 上启用缓存；

## SWR 策略

- 过期后先回旧值，再后台刷新；
- 记录 `stale_served_total` 与 `age_ms`；
- Key 生成与租户隔离遵循 SB‑16 的键规范；

## 失效接线

- 订阅 SB‑09/10 的失效事件，按 `tenant/namespace/resource` 前缀清理相关条目；
- 失败重试与幂等保护。
