# Sandbox 基线（Issue 05 对齐）

> 在不改变原文结构的前提下，补充默认拒绝、能力白名单、Consent 门禁与安全控制基线，统一错误码映射与验收口径。

## 基本原则

- 默认拒绝（deny-by-default）：未明确声明或授权的能力，一律拒绝。
- 最小权限：能力按最小粒度授予（按资源/动作/范围/限制条件）。
- 可审计：所有放行/拒绝均产生 Evidence（摘要化），对外仅公共视图。

## 能力白名单（示例分类）

- 网络：`net.http.{get,head}`（默认仅公网上行；需显式允许域名/端口/协议）；
- 文件：`fs.read:{scope}`、`fs.write:{scope}`（限制到沙箱根与允许的子目录；禁止符号链接逃逸）；
- 进程：`process.spawn:{cmd}`（默认禁用；仅允许内建安全子命令且限资源）；
- 其它：`browser.fetch`（等同于严格版 `net.http.get`）；`crypto.hash`；`image.convert`（限制格式/尺寸/时间）。

## Consent 门禁（高风险能力）

- 需要 Consent 的能力（含示例）：
  - `fs.write:*`、`process.spawn:*`、`net.http.post|put|patch|delete`；
  - 可能外传数据或持久化副作用的操作；
- 校验路径：`Consent` 由上游（SB‑04/05/08）透传，Sandbox 在执行前二次校验（租户/主体/范围/过期）。
- 不满足时返回：`SANDBOX.PERMISSION_DENIED`（SB‑02）。

## 安全控制

- 路径归一化：
  - 归一化后必须在允许的根目录内；
  - 拒绝 `..` 穿越、拒绝跟随越界符号链接；
- SSRF/重定向：
  - 默认拒绝私网/链路本地/环回/元数据地址（IPv4/IPv6 覆盖）；
  - 禁止跨到私网的 30x 重定向；
  - TLS 强制与证书校验；
- 资源上限：
  - 时间（wall/cpu）/内存/并发/输出体积/总字节数/最大图片尺寸等可配置上限；
  - 超限时终止并返回 `SANDBOX.CAPABILITY_BLOCKED`（描述为“limit exceeded”）；
- 协议与方法：
  - 网络仅允许 `GET/HEAD`；其它方法需能力+Consent；
  - 禁止 file://、ftp://、gopher:// 等非白名单协议。

## 错误码（SB‑02 映射）

- 未授权能力/Grant 缺失/过期/撤销 → `SANDBOX.PERMISSION_DENIED`；
- 能力被策略阻断/超过安全阈值/不在白名单 → `SANDBOX.CAPABILITY_BLOCKED`；
- 请求/参数非法（路径非法、协议不被支持）→ `SCHEMA.VALIDATION_FAILED`。

## 观测与证据

- Evidence（Begin/End）记录：能力、资源范围、参数摘要、`config_version/hash`、耗时与资源用量摘要；
- 日志脱敏：不写入原文数据，仅存 `digest/ref`；
- 指标：拒绝/放行次数、超限次数、私网拦截命中数等。

## 验收（负向用例建议）

- 访问 169.254.169.254 / 10.0.0.0/8 / fd00::/8 → 被拒（SSRF 拦截）；
- HTTP 30x 跳转到私网 → 被拒；
- `../../etc/passwd` 路径穿越 → 被拒；
- 未携带 Consent 的 `fs.write` / `net.http.post` → 被拒；
- 超过时间/内存/体积/尺寸上限 → 终止并返回 `SANDBOX.CAPABILITY_BLOCKED`；
- 所有拒绝对外仅公共视图（`to_public()`），审计信息写入观测。
