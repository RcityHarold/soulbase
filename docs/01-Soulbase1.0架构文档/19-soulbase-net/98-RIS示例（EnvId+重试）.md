# RIS 示例（EnvId 注入 + 幂等方法重试）

> 目标：演示在 RIS 层如何为出站请求注入 `X-Env-Id`，并仅对幂等方法启用重试；与 Issue 02/06/07 的约定一致。

```rust
use soulbase_net::prelude::*;

/// 简单的 EnvId 注入拦截器
dyn_interceptor! { EnvIdHeader, before(req: NetRequest) -> NetRequest {
    let mut req = req;
    // 从上游上下文取得 envelope_id（示例硬编码）
    let env_id = req.headers.iter().find(|(k,_)| k.eq_ignore_ascii_case("x-env-id"))
        .map(|(_,v)| v.clone()).unwrap_or_else(|| "env_demo_v7".into());
    if !req.headers.iter().any(|(k,_)| k.eq_ignore_ascii_case("x-env-id")) {
        req.headers.push(("X-Env-Id".into(), env_id));
    }
    req
} }

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut policy = NetPolicy::default();
    // 幂等重试策略：仅对 GET/HEAD 生效；遵守 Retry-After
    policy.retry.enabled = true;
    policy.retry.max_attempts = 3;
    let client = ClientBuilder::default()
        .with_policy(policy)
        .with_interceptor(TraceUa::default())
        .with_interceptor(EnvIdHeader)
        .build();

    // 幂等 GET，将触发自动重试
    let req = NetRequest { method: http::Method::GET, url: "https://example.com/etag".into(), ..Default::default() };
    let resp = client.send(req).await?;
    println!("status={}, headers={:?}", resp.status, resp.headers);

    Ok(())
}
```

要点：
- `EnvIdHeader` 在 `before()` 阶段补 `X-Env-Id`；
- `NetPolicy.retry` 启用后，仅对幂等方法（GET/HEAD…）生效；
- 与 Issue 07 的 CacheHook/SWR 不冲突（只读路径）。
