# Config 快照贯穿（Issue 04 对齐）

> 明确账页（Ledger）记录配置快照的要求。

## 账页与回执

- Ledger 行项目包含 `config_version/hash` 字段（用于对账/回放）；
- 回执（Receipt）对象包含 `config_version/hash`，与 Evidence/响应头一致；
- 旧请求在热更期间按照请求开始时的快照计费与结算。

## 验收

- 账页/回执/Evidence/响应头四处快照一致；
- 热更期间旧请求不漂移；
- Contract-TestKit 覆盖“热更不漂移/一致性校验”用例。
