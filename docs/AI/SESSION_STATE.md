# 会话状态记录 (SESSION STATE)

本文档记录当前开发会话的状态，是跨 Session 恢复工作的直接依据。

---

## 1. 核心状态概要

- **当前 Goal**: 系统全面精简与性能飞跃重构（收敛为单一 SQLite + ClickHouse 架构并建设统一备份体系与标准部署体系）
- **当前 Task**: 
  - **TASK-015**: ClickHouse 系统日志轻量化抑制与写入碎片（Parts）合并防堵调优 (`DONE`)
- **当前状态**: `DONE` (已验证 DDL 设置、客户端 async_insert/log_queries 动态注入、服务端 system_logs/tuning 配置挂载、compose 校验与全部 55 项单元及集成测试通过)

---

## 2. 本次会话完成内容 (Accomplished Work)

### 2.1 写入碎片防堵调优 (Parts Anti-Blocking / Async Insert)
- **底层根治客户端微批写入 Parts 爆炸**：在 [`src/clickhouse/mod.rs`](file:///ssd0/git/EVMEventLake/src/clickhouse/mod.rs) 中为客户端全局启用 `async_insert = 1`、`wait_for_async_insert = 1` 与 `async_insert_busy_timeout_ms = 200`。服务端在内存中自动将高并发微批聚合成健康大 Part 再落盘，彻底消除 `Too many parts` 异常与 `parts_to_delay_insert` 强制写入延迟，且保证了 Checkpoint 推进的一致性。
- **表级 DDL 参数放宽 (`clickhouse/schema.sql`)**：为 `raw_logs`、`blocks` 和 `transactions` 表的 `SETTINGS` 配置 `parts_to_delay_insert = 300`、`parts_to_throw_insert = 600`、`max_delay_to_insert = 1`，提升高压下的缓冲空间。
- **配置化驱动 (`src/configuration/mod.rs`)**：`ClickHouseConfig` 支持 `EVENTLAKE_CLICKHOUSE_ASYNC_INSERT` 与 `EVENTLAKE_CLICKHOUSE_WAIT_FOR_ASYNC_INSERT` 环境变量覆盖。

### 2.2 系统操作日志轻量化抑制 (System Logs Suppression)
- **源头切断查询日志放大**：客户端写入与 Profile 默认设置 `log_queries = 0`，避免数据管道千万级 INSERT 微批污染膨胀 `system.query_log`。
- **生命周期截断与高开销日志移除**：新建 [`clickhouse/config.d/system_logs.xml`](file:///ssd0/git/EVMEventLake/clickhouse/config.d/system_logs.xml) 与 [`clickhouse/users.d/tuning.xml`](file:///ssd0/git/EVMEventLake/clickhouse/users.d/tuning.xml)：
  - 彻底移除采样追踪日志 `<trace_log remove="1"/>`。
  - 将 `query_log`、`part_log`、`text_log`、`metric_log` 的 TTL 统一截断为 1~2 天自动淘汰。
  - 提高后台合并并发线程池 `<background_pool_size>16</background_pool_size>`。
- **容器化标准挂载**：在 [`docker-compose.yml`](file:///ssd0/git/EVMEventLake/docker-compose.yml) 与 [`docker-compose.source.yml`](file:///ssd0/git/EVMEventLake/docker-compose.source.yml) 中挂载 `./clickhouse/config.d` 与 `./clickhouse/users.d` 至容器 `/etc/clickhouse-server/`。

### 2.3 固化至 AI 事实来源知识库
- 新增 **[`docs/AI/DECISIONS.md`](file:///ssd0/git/EVMEventLake/docs/AI/DECISIONS.md) -> ADR-008: ClickHouse 写入碎片（Parts）防堵塞调优与系统日志轻量化治理**。
- 更新 **[`docs/AI/ARCHITECTURE.md`](file:///ssd0/git/EVMEventLake/docs/AI/ARCHITECTURE.md)**：第 3.1 & 3.2 节补充写入分片防堵与系统操作日志轻量化架构规范。

---

## 3. 文件变动清单

### 新建文件 (Created Files)
- `clickhouse/config.d/system_logs.xml`: ClickHouse 服务端日志 TTL 截断、移除 trace_log 与后台合并线程调优。
- `clickhouse/users.d/tuning.xml`: ClickHouse 用户 Profile 默认开启 async_insert 与 log_queries=0。
- `docs/AI_CLICKHOUSE_DIRECTIVE.md`: 面向 AI Agent 的 ClickHouse 容器化调优与工程审查通用指令规范（可跨项目直接提供给 AI 使用）。
- `docs/CLICKHOUSE_OPTIMIZATION_GUIDE.md`: ClickHouse 生产级全维度调优与避坑指南（Docker 容器化专属增强版）。
- `docs/AI/tasks/TASK-015.md`: TASK-015 任务定义与完成记录。

### 调整修正的文件 (Modified Files)
- `clickhouse/schema.sql`: 为 `raw_logs`、`blocks`、`transactions` 表追加 `parts_to_delay_insert`、`parts_to_throw_insert`、`max_delay_to_insert` 表级设置。
- `src/clickhouse/mod.rs`: `connect` 逻辑中为 Client 全局注入 `async_insert=1`、`wait_for_async_insert=1`、`log_queries=0`。
- `src/configuration/mod.rs`: 扩展 `ClickHouseConfig` 字段及环境变量支持与测试用例。
- `docker-compose.yml`: 挂载 `./clickhouse/config.d` 与 `./clickhouse/users.d`。
- `docker-compose.source.yml`: 挂载 `./clickhouse/config.d` 与 `./clickhouse/users.d`。
- `tests/clickhouse_integration_tests.rs`: 适配 `ClickHouseConfig` 构造。
- `tests/live_real_evm_data_tests.rs`: 适配 `ClickHouseConfig` 构造。
- `docs/AI/DECISIONS.md`: 登记 ADR-008。
- `docs/AI/ARCHITECTURE.md`: 新增 3.1 & 3.2 节架构规范。
- `docs/AI/TASK_INDEX.md`: 看板更新 TASK-015 为 `DONE`。

---

## 4. 已运行的验证命令及结果

```bash
# 1. 验证 Docker Compose 挂载配置合法性
docker compose --env-file .env.example -f docker-compose.yml config > /dev/null
docker compose --env-file .env.example -f docker-compose.source.yml config > /dev/null
# 输出: 退出码 0 (配置均合法无误)

# 2. 验证 Rust 静态类型与全部 target 编译
cargo check --ignore-rust-version --all-targets
# 输出: Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.93s (退出码 0)

# 3. 运行完整单元与集成测试套件
cargo test --ignore-rust-version
# 输出: 29 unittests + 6 block_transaction + 2 clickhouse_integration + 2 collector_concurrency + 3 e2e_real_database + 6 live_real_evm_data + 3 rpc_pool_cooldown + 1 search_dsl + 3 validation = 55 passed; 0 failed (退出码 0)
```

---

## 5. 未解决问题 (Known Issues)

- 无。

---

## 6. 风险和假设 (Risks and Assumptions)

- **假设**: 外部独立的 ClickHouse 部署环境（非 Compose 部署）通过客户端注入的 `async_insert = 1` 选项同样可以享受服务端攒批与免日志特性。
- **风险**: 无破坏性风险，所有行为均向后兼容并可通过环境变量或配置微调。

---

## 7. 下一步计划 (Next Task)

- **建议**: 将当前所有的重构与 ClickHouse 调优成果提交 Git。
- **下一次 Session 应先读取的文件**:
  1. [`AGENTS.md`](file:///ssd0/git/EVMEventLake/AGENTS.md)
  2. [`docs/AI/GOAL.md`](file:///ssd0/git/EVMEventLake/docs/AI/GOAL.md)
  3. [`docs/AI/TASK_INDEX.md`](file:///ssd0/git/EVMEventLake/docs/AI/TASK_INDEX.md)
  4. [`docs/AI/SESSION_STATE.md`](file:///ssd0/git/EVMEventLake/docs/AI/SESSION_STATE.md)
