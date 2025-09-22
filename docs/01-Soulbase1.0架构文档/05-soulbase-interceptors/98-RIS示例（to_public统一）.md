# RIS 示例（统一公共错误视图 to_public）

> 目标：演示拦截器链如何仅返回公共错误视图，隐藏开发细节；与 Issue 01/06 对齐。

```rust
use soulbase_errors::prelude::*;
use axum::{Json, response::IntoResponse};

fn map_err_to_http(err: ErrorObj) -> impl IntoResponse {
    // 仅公共视图对外
    let public = err.to_public();
    let status = soulbase_errors::mapping_http::to_http_status(&err);
    (status, Json(public))
}

pub async fn handler() -> Result<Json<serde_json::Value>, ErrorObj> {
    // 示例：参数非法 → 统一为 SCHEMA.VALIDATION_FAILED
    Err(ErrorBuilder::new(codes::SCHEMA_VALIDATION)
        .user_msg("Request is invalid.")
        .dev_msg("missing field: name")
        .build())
}
```

要点：
- 所有错误经 `ErrorObj` 构造，并在适配器处调用 `to_public()`；
- `message_dev/meta/cause_chain` 仅写入审计，不对外透出；
- HTTP/gRPC 状态遵循 SB‑02 的映射函数。
