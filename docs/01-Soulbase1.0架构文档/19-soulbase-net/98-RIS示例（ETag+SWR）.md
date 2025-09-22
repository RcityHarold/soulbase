# RIS 示例（ETag + SWR 条件请求）

> 目标：演示在 GET 路径上结合 ETag 条件请求与 SWR 的最小逻辑；与 Issue 07 对齐。

```rust
use soulbase_net::prelude::*;

async fn get_with_swr(client:&ReqwestClient, url:&str, etag_cache:&mut Option<String>) -> anyhow::Result<NetResponse> {
    let mut req = NetRequest{ method:http::Method::GET, url:url.into(), ..Default::default() };
    if let Some(tag) = etag_cache.as_ref() {
        req.headers.push(("If-None-Match".into(), tag.clone()));
    }
    let resp = client.send(req).await?;
    if resp.status == 304 {
        // SWR：返回本地缓存（此处省略），同时刷新 TTL
        // ...
    } else if let Some(tag) = resp.headers.iter().find(|(k,_)| k.eq_ignore_ascii_case("etag")).map(|(_,v)| v.clone()) {
        *etag_cache = Some(tag);
    }
    Ok(resp)
}
```

要点：
- 仅在 GET/HEAD 路径使用 CacheHook/SWR；
- 命中 304 时返回旧值并后台刷新；
- Key/租户隔离遵循 SB‑16 的键规范；
- 写后强失效由 Storage/Tx 事件驱动（见 Issue 07）。
