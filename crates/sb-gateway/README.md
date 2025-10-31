# sb-gateway

`sb-gateway` 是 Soulbase 薄腰能力的示例 HTTP 网关，实现了以下能力：

- `GET /healthz` 健康检查；
- `POST /tenants/{tenant_id}/tools.execute`：接收工具计划并返回规范化的工具执行结果；
- `POST /tenants/{tenant_id}/collab.execute`：返回协作分叉所需的上下文与占位结果；
- 统一接入 `sb-interceptors`，提供请求追踪、响应头透传与错误结构化输出。

当前实现以内置模拟逻辑响应请求，便于与 `soulseed-agi-ace` 联调验证真实数据链路。后续可将工具/协作的核心逻辑替换为真实的 `sb-tools`、`sb-auth`、`sb-storage` 等模块。

## 配置

通过环境变量控制网关行为（见 `.env.example`）：

| 变量 | 默认值 | 说明 |
| --- | --- | --- |
| `SB_GATEWAY_ADDR` | `0.0.0.0:8800` | 网关监听地址 |

> 其余如鉴权、存储、工具执行等依赖可按需追加，建议放入独立的配置文件并在后续版本中集成。

## 运行

```bash
cd crates/sb-gateway
cargo run
```

项目启动后，可使用以下示例请求测试：

```bash
curl -X POST http://127.0.0.1:8800/tenants/1/tools.execute \
  -H "Content-Type: application/json" \
  -d '{
    "plan": {
      "nodes": [
        {"id": "n1", "tool_id": "web.search", "input": {"query": "hello world"}}
      ],
      "barrier": {"mode": "all"}
    }
  }'
```

## 下一步

- 接入真实工具注册表与执行引擎；
- 增加幂等存储、鉴权、配额管控等拦截器；
- 扩展 LLM、Graph 等薄腰接口，使其与 `soulseed-agi-ace` 的其它分叉保持一致。
