# sb-errors

统一错误域与稳定错误码，提供跨协议映射与最小披露视图。

## 构建

- cargo check
- cargo test

## 可选特性

- http: 转换为 http::StatusCode
- grpc: 转换为 tonic::Status
- wrap-reqwest / wrap-sqlx: 封装外部错误
