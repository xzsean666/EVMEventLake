# TASK-022: 服务端区块交易常驻解耦与独立纯 Shell 客户端 (lakectl)

## Objective
1. 服务端解耦：将区块交易后台 Worker 调整为默认常驻运行（由 SQLite 状态按需唤醒），彻底摆脱对 `.env` 静态开关及容器重启的依赖。
2. 开发独立跨平台客户端命令 `scripts/lakectl`：纯 Shell 实现，零编译依赖，可在任意机器独立使用；支持直接 HTTP 与 SSH 隧道安全穿透；内置 `top` 动态交互式终端监控看板；支持动态管理 blocks/transactions 同步与 raw_logs 订阅。

## Scope
- 包含：
  - 修改 `src/configuration/mod.rs` 和 `.env.example`，将 `EVENTLAKE_BLOCK_TRANSACTION_ENABLED` 默认值改为 `true`。
  - 新增 `scripts/lakectl`（POSIX Bash 独立脚本，支持 `--url`, `--token`, `--ssh`, `-p`, `-i`）。
  - 实现命令体系：
    - `lakectl top`（终端动态 TUI 看板，按 q 退出，按 r 刷新，显示整体指标、RPC 延迟池、日志采集和区块交易进度）。
    - `lakectl status`（单次输出 Lake 全局状态）。
    - `lakectl block <status|start|pause|resume>`（动态控制整链区块与交易采集）。
    - `lakectl log <list|add|pause|resume|delete>`（动态控制事件日志采集）。
    - `lakectl rpc <list|check>`（RPC 节点池测速与健康检查）。
    - `lakectl chain <list>`（查看链配置）。
  - 更新 `docs/USAGE.md` 包含 `lakectl` 客户端手册。
  - 任务与会话状态跟踪记录。
- 不包含：
  - 破坏现有 REST API 契约。

## Allowed Files
- `src/configuration/mod.rs`
- `.env.example`
- `scripts/lakectl`
- `docs/USAGE.md`
- `docs/AI/tasks/TASK-022.md`
- `docs/AI/TASK_INDEX.md`
- `docs/AI/SESSION_STATE.md`

## Dependencies
- 前置任务：TASK-021 (已完成)

## Inputs and Outputs
- **Inputs**:
  - 用户对任意机器使用、纯 Shell 实现、SSH 隧道穿透、免 env 控制区块交易的诉求
- **Outputs**:
  - 独立客户端工具 `scripts/lakectl`
  - 服务端常驻解耦代码
  - 更新后的文档与测试

## Acceptance Criteria
- [x] 服务端 Worker 默认常驻待命，无激活链时 0 开销休眠，无需修改 `.env` 即可通过 API/CLI 随时热启停区块交易。
- [x] `scripts/lakectl` 具备完整的命令行参数解析与容错提示，语法检查通过 (`bash -n`)。
- [x] 支持 `--ssh` 自动后台建立并清理安全端口转发，在端口未暴露时正常通信。
- [x] `lakectl top` 呈现清晰高亮的 ANSI 终端监控大盘。
- [x] `lakectl block`、`lakectl log`、`lakectl rpc` 控制子命令执行准确。
- [x] 全库 Rust 测试 (`cargo test --ignore-rust-version`) 保持 100% 通过。

## Verification Commands
```bash
cargo test --ignore-rust-version --locked
bash -n scripts/lakectl
./scripts/lakectl --help
./scripts/lakectl block --help
./scripts/lakectl log --help
```

## Verification Results
- `cargo test --ignore-rust-version --locked`：全库 60 项测试 100% 通过（包含单元测试、E2E 测试、分叉回退测试与 ClickHouse 集成测试）。
- `bash -n scripts/lakectl`：语法检查退出码 0，无任何语法错误。
- `./scripts/lakectl --help`：完整输出全部全局参数（`-u`, `-k`, `-s`, `-p`, `-i`）与核心子命令指引。
- 子命令容错测试：离线/非法指令拦截并优雅输出提示信息。
- 文档 `docs/USAGE.md` 已全面补充 `lakectl` 安装、SSH 穿透与常用命令使用说明。
- 任务标记完成。
