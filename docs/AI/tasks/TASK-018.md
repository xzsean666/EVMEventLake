# TASK-018: 全面修复 ClickHouse 容器启动故障与端到端 (E2E) 测试缺陷并补齐完整端到端测试链路

## Objective
修复 ClickHouse 容器因配置语义与只读目录挂载导致的启动失败；修复现有公网端到端测试 `live_chain_collects_and_searches_raw_base_usdc_logs` 中的 ClickHouse 未注入与表查询笔误；补齐基于本地 Mock RPC Fixture + 真实 SQLite + 真实 ClickHouse 的全链路闭环 E2E 自动化测试（含 Raw Logs 采集与检索、区块交易同步与 Reorg 联动）；并在备份恢复脚本测试中打通 ClickHouse 验证。

## Scope
- 包含：
  - `clickhouse/users.d/tuning.xml`：清理 `<profiles>` 下误用的 MergeTree 表级参数，消除 ClickHouse 24.8 启动致命异常 `UNKNOWN_SETTING: max_delay_to_insert`。
  - `docker-compose.yml`：将 `config.d` 与 `users.d` 目录挂载调整为精确文件挂载，避免掩盖默认端口监听配置并允许 ClickHouse entrypoint 自动生成 `default-user.xml`。
  - `tests/e2e_real_database_tests.rs`：修复 `live_chain_collects_and_searches_raw_base_usdc_logs` 中 ClickHouse 客户端注入与日志表行数统计断言。
  - `tests/e2e_real_database_tests.rs`：新增或完善本地闭环全流程端到端测试（Mock RPC -> Collector -> ClickHouse -> Search DSL API -> Reorg 回退），无需公网连接即可 100% 稳定运行。
  - `tests/test_backup_restore_e2e.sh`：支持在 ClickHouse 在线时自动验证 ClickHouse 数据湖的分片备份与恢复。
- 不包含：
  - 核心业务采集协议变更或破坏性重构。
  - 外部依赖库升级或添加不必要的新依赖。

## Allowed Files
- `clickhouse/users.d/tuning.xml`
- `docker-compose.yml`
- `tests/e2e_real_database_tests.rs`
- `tests/test_backup_restore_e2e.sh`
- `docs/AI/TASK_INDEX.md`
- `docs/AI/SESSION_STATE.md`
- `docs/AI/tasks/TASK-018.md`

## Dependencies
- 前置任务：TASK-017
- 外部依赖：Docker 环境（用于验证 ClickHouse 容器启动与集成测试）

## Inputs and Outputs
- **Inputs**:
  - ClickHouse 官方镜像 24.8 的配置规范与 Entrypoint 行为。
  - 本地 Mock RPC Fixture 与真实 EVM 数据采集协议。
- **Outputs**:
  - 可正常健康的 ClickHouse 容器化编排环境。
  - 100% 通过的本地闭环与链上端到端测试套件。

## Acceptance Criteria
- [x] 标准 1：`docker compose up -d clickhouse` 能够正常拉起且健康检查变为 healthy，`curl http://127.0.0.1:8123/ping` 返回 200 OK。
- [x] 标准 2：修复 `live_chain_collects_and_searches_raw_base_usdc_logs`，在配置 `EVENTLAKE_RUN_LIVE_CHAIN_E2E=true` 时执行通过且不发生 Panic。
- [x] 标准 3：提供完全脱离公网的本地闭环端到端测试，验证从 Mock RPC 接收日志 -> ClickHouse 真实写入 -> `/api/raw-logs/search` 检索过滤 -> 区块交易同步 -> Reorg 分叉回退的全链路。
- [x] 标准 4：所有既有单元测试、集成测试及备份恢复脚本保持 100% 通过。

## Verification Commands
```bash
# 1. 验证 ClickHouse 容器健康启动
docker compose up -d clickhouse
docker compose ps clickhouse
# 结果: Up (healthy), HTTP 200 OK

# 2. 运行完整单元与集成测试（含修复后的 E2E 测试及公网测试）
EVENTLAKE_RUN_CLICKHOUSE_INTEGRATION=true EVENTLAKE_RUN_LIVE_CHAIN_E2E=true EVENTLAKE_CLICKHOUSE_URL="http://eventlake:eventlake@127.0.0.1:8123/eventlake" cargo test --ignore-rust-version -- --nocapture
# 结果: 57 passed, 0 failed

# 3. 运行端到端备份恢复测试（包含 SQLite 与 ClickHouse 联动）
./tests/test_backup_restore_e2e.sh
# 结果: All Backup and Restore E2E Verification Tests PASSED!
```

## Risks and Assumptions
- 风险：极低，均为测试健壮性修复与容器化配置规范对齐，不变更任何对外 HTTP 接口与业务模型。
- 假设：本地 Docker 服务可用（端口 8123 / 9000 未被其他非 ClickHouse 进程冲突占用）。

## Status
DONE
