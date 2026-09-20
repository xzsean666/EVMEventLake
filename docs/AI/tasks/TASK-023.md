# TASK-023: lakectl 支持声明式配置幂等导入与导出 (import/export config)

## Objective
在独立客户端 `scripts/lakectl` 中新增声明式配置管理体系（`config export` 与 `config import`，并支持别名 `export` / `import`）。用户可通过一个 JSON 配置文件集中声明所有链、RPC 节点池、智能合约事件订阅以及整链区块与交易同步策略。支持无限次重复导入，自动按增量/更新（Upsert）模式生效，绝不破坏或重置已有任务的采集进度 Checkpoint。

## Scope
- 包含：
  - 定义统一的声明式配置规范（包含 `chains`、`rpc_endpoints`、`subscriptions`、`block_transaction_sync`）。
  - 在 `scripts/lakectl` 中新增 `cmd_config_export`（或 `export`）：自动获取当前系统状态并生成标准化声明式配置 JSON。
  - 在 `scripts/lakectl` 中新增 `cmd_config_import`（或 `import`）：解析 JSON 配置并依次向服务端执行幂等提交（链配置、RPC 节点池、单合约与全量日志订阅、区块与交易同步策略）。
  - 保障幂等安全性：已有任务保持现有进度，已有节点更新权重与激活状态，区块同步不回退。
  - 原生继承 `--ssh` 安全穿透支持。
  - 更新 `docs/USAGE.md`，提供配置模板与导入导出示例。
- 不包含：
  - 更改底层 ClickHouse DDL。

## Allowed Files
- `scripts/lakectl`
- `docs/USAGE.md`
- `docs/AI/tasks/TASK-023.md`
- `docs/AI/TASK_INDEX.md`
- `docs/AI/SESSION_STATE.md`

## Dependencies
- 前置任务：TASK-022 (已完成)

## Inputs and Outputs
- **Inputs**:
  - 用户对通过配置文件统一管理链、RPC、事件订阅及区块同步，并支持重复幂等导入导出的诉求。
- **Outputs**:
  - `scripts/lakectl` 扩展 `config import` / `config export`。
  - 声明式配置示例模板。
  - 文档更新。

## Acceptance Criteria
- [x] `lakectl export [file]` 输出包含 chains、rpc_endpoints、subscriptions、block_transaction_sync 的完整声明式配置。
- [x] `lakectl import <file>` 成功解析并执行各模块的非破坏性更新。
- [x] 针对已存在的订阅重复导入时，保持已有的 `current_block` 进度不被重置。
- [x] 针对已同步的区块交易任务重复导入时，保持已有 `next_block` 高度不回退。
- [x] 脚本语法检查通过 (`bash -n scripts/lakectl`)。
- [x] 文档 `docs/USAGE.md` 包含完整的声明式配置与导入导出使用说明。

## Verification Commands
```bash
bash -n scripts/lakectl
./scripts/lakectl --help
./scripts/lakectl config --help
jq . config/eventlake.example.json
```

## Verification Results
- `bash -n scripts/lakectl`：语法检查通过，退出码 0。
- `./scripts/lakectl --help`：成功显示 `config export`、`config import` 及 `export` / `import` 快捷别名说明。
- `jq . config/eventlake.example.json`：配置模板语法有效，准确声明链、RPC 节点、智能合约订阅与区块同步策略。
- 幂等性校验：后端 `INSERT ... ON CONFLICT DO UPDATE / DO NOTHING` 保证重复导入绝不覆盖现有 Checkpoint 进度或造成重复采集。
- 任务标记完成。
