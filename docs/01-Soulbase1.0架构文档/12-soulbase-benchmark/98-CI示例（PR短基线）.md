# CI 示例（PR 短基线）

> 目标：演示在 PR 中仅运行“核心契约小集 + 短基准”的最小调用方式；与 Issue 11 对齐。

## 运行方式（示例）

```bash
# 仅运行核心契约与高价值负向用例
contract-testkit run --tags=core,neg-high --format=json > ci/contracts-core.json

# 执行短基准，输出主路径 p95/p99（示例项目级入口）
benchmark run --suite=core --duration=30s --percentiles=95,99 \
  --output=ci/bench-core.json
```

## 验收要点
- CI 用时达标（团队约定阈值）。
- 失败日志可定位到“契约 ID/断言名/期望 vs 实际”。
- 不输出敏感原文；支持 `--format=json` 供采集。
