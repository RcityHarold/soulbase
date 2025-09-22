# Config 快照贯穿（Issue 04 对齐）

> 明确 A2A 回执/证据的配置快照记录要求，保证跨域对齐。

## 回执与证据

- A2A `Receipt`/`Ack` 中包含 `config_version/hash`，便于双方对账与重放校验；
- 验签/反重放逻辑不依赖配置快照，但回执与账页需与本地 Evidence/响应头一致；
- 跨域差异：若双方版本不一致，应在回执中明确双方 `config_version` 用于差异定位。

## 验收

- 双边回执均携带 `config_version/hash`，且与各自 Evidence/响应头一致；
- 跨域对账以 `envelope_id`+`config_version` 为维度可定位；
- Contract-TestKit 覆盖“版本不一致的回执差异定位”用例建议。
