# 错误码附录（Issue 01 对齐）

> 本附录对应《01‑SoulBase总体19模块精检文档》Issue 01 要求，列出需新增/规范的码位及其建议映射。完整清单以 SB‑02 RIS 中的注册表为准。

## 新增码位（Tx/A2A/Sandbox）

| code | kind | http_status | grpc_status | retryable | severity |
| ---- | ---- | ----------- | ----------- | --------- | -------- |
| `TX.TIMEOUT` | Timeout | 504 | DEADLINE_EXCEEDED | Transient | Error |
| `TX.IDEMPOTENT_BUSY` | Conflict | 409 | ABORTED | Transient | Warn |
| `TX.IDEMPOTENT_LAST_FAILED` | Conflict | 409 | ABORTED | None | Error |
| `A2A.REPLAY` | A2AError | 409 | ALREADY_EXISTS | None | Warn |
| `A2A.CONSENT_REQUIRED` | A2AError | 428 | FAILED_PRECONDITION | Permanent | Warn |
| `A2A.LEDGER_MISMATCH` | A2AError | 409 | ABORTED | None | Error |
| `SANDBOX.CAPABILITY_BLOCKED` | Sandbox | 403 | PERMISSION_DENIED | None | Warn |

## 统一拼写/命名（兼容别名）

- `SCHEMA.VALIDATION_FAILED` 为规范名；历史拼写 `SCHEMA_VAILDATION` 属兼容别名（不建议使用）。
- `SANDBOX.PERMISSION_DENIED` 为规范名；历史名 `SANDBOX.PERMISSION_DENY` 属兼容别名（不建议使用）。

> 协议映射为建议值；上层可在不改变语义的前提下合法降级/升级，必要时可加 `Retry-After`。

