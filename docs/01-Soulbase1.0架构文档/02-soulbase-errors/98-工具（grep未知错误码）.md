# 工具（grep 未知错误码）

> 目标：在 CI 或本地快速发现“未注册/拼写错误”的错误码引用；与 Issue 01/09 对齐。

## ripgrep 示例

```bash
# 列出项目中所有疑似错误码引用（大写.大写 形式），排除已知注册处
rg -n "[A-Z_]+\.[A-Z0-9_]+" \
  --glob '!**/target/**' --glob '!**/node_modules/**' \
  | rg -v "soulbase-errors|REGISTRY|codes::|99-错误码附录"

# 与 SB-02 REGISTRY 比对（示意）：
# 1) 导出 REGISTRY 码表列表（在 RIS 或脚本中输出到 files/known_codes.txt）
# 2) 用 comm 或 diff 发现“引用但未注册”的码位
comm -23 <(sort files/all_codes.txt) <(sort files/known_codes.txt) > files/unknown_codes.txt

# 若 unknown_codes.txt 非空 → CI 失败并给出具体码值
```

要点：
- 统一从 SB‑02 的 REGISTRY 导出“已知码位清单”；
- 规则基于约定格式（大写.大写/数字/下划线），若项目有其它命名需在脚本中加入白名单；
- 与 SB‑11 的覆盖率指标协同：unknown 视为覆盖率缺口。
