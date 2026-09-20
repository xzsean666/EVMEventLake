# TASK-017: 核心安全加固与高危性能缺陷治理（RPC SSRF/重定向防御、Dashboard FINAL 慢查询消除、raw_logs 合约地址跳数索引补齐、收据异常重试保护与恢复脚本防坏治理）

## Objective
针对全面审计中发现的高危与关键缺陷进行闭环治理：修复 RPC 节点池 SSRF 域名解析与 HTTP 30x 重定向绕过；消除 `/api/dashboard` 中 ClickHouse 全表 `FINAL` 统计引发的 OOM 隐患；补齐 `raw_logs` 的 `contract_address` Bloom Filter 索引；修复区块收据（Receipts）在网络抖动时被永久静默丢弃的数据质量缺陷；并加固 `restore.sh` / `backup.sh` 防止热恢复损坏 SQLite WAL 数据库。

## Scope
- 包含：
  1. `src/app/application_state.rs`：HTTP 客户端禁用重定向（`redirect(Policy::none())`）。
  2. `src/rpc_pool/mod.rs`：`validate_rpc_url_ssrf` 增加域名同步 DNS 解析与解析后 IP 的私网/回环检查，支持 IPv6 映射解析，补齐单元测试。
  3. `src/clickhouse/mod.rs`：`raw_log_count` 改为从 `system.parts` 零开销获取行数统计，杜绝全表跨分区 `FINAL` 归并。
  4. `clickhouse/schema.sql`：为 `raw_logs` 表增加 `contract_address` 的 Bloom Filter 跳数索引。
  5. `src/block_transaction/collector.rs`：收据批量获取失败时严格区分 RPC 不支持与网络异常，网络异常时安全中断当前 Tick 并重试，杜绝数据永久静默丢失。
  6. `scripts/restore.sh` & `scripts/backup.sh`：检测进程存活以阻止热恢复，清理残留 `-wal` 和 `-shm`，加固 VACUUM 参数化调用。
- 不包含：
  - 不修改外部公开 API 契约与路由路径。
  - 不破坏现有 55 项单元和集成测试。

## Allowed Files
- `src/app/application_state.rs`
- `src/rpc_pool/mod.rs`
- `src/clickhouse/mod.rs`
- `clickhouse/schema.sql`
- `src/block_transaction/collector.rs`
- `scripts/restore.sh`
- `scripts/backup.sh`
- `docs/AI/tasks/TASK-017.md`
- `docs/AI/TASK_INDEX.md`
- `docs/AI/SESSION_STATE.md`

## Dependencies
- 前置任务：TASK-016
- 外部依赖：None

## Inputs and Outputs
- **Inputs**: 审计发现的 SSRF 绕过点、ClickHouse 跨分区全表 FINAL 统计点、收据异常丢弃逻辑、恢复脚本热覆盖点。
- **Outputs**: 加固后的网络客户端、域名 SSRF 防御校验器、ClickHouse 轻量化统计、带有合约地址跳数索引的 ClickHouse Schema、收据可靠采集重试流程与安全的恢复脚本。

## Acceptance Criteria
- [x] 1. **SSRF 与重定向防御**：`reqwest::Client` 明确禁止跟随 HTTP 重定向；RPC 节点 URL 的域名解析到私有/回环 IP 时被明确拒绝并拦截。
- [x] 2. **Dashboard 轻量化统计**：`raw_log_count` 不再执行 `SELECT count() FROM raw_logs FINAL`，改为从 `system.parts` 查询，消除高负载 OOM 隐患。
- [x] 3. **合约地址索引补齐**：`clickhouse/schema.sql` 中 `raw_logs` 显式声明 `raw_logs_address_idx contract_address TYPE bloom_filter(0.01) GRANULARITY 4`。
- [x] 4. **收据网络异常保护**：`eth_get_block_receipts_batch` 在返回网络/超时错误时中断当前批次采集并记录 RPC failure，不推进 Checkpoint，避免收据永久缺失。
- [x] 5. **脚本安全加固**：`restore.sh` 在检测到 `eventlake` 正在运行时阻止直接热恢复，并在拷贝前清除 `-wal` 与 `-shm` 避免数据库损坏；`backup.sh` 使用参数化 `VACUUM INTO ?`。
- [x] 6. **测试全量通过**：静态检查与所有 56 项单元/集成测试及备份恢复 E2E 验证 100% 通过。

## Verification Commands
```bash
cargo check --ignore-rust-version --all-targets
cargo test --ignore-rust-version
./tests/test_backup_restore_e2e.sh
```

## Verification Results
- `cargo check --ignore-rust-version --all-targets`: 通过 (2.87s, 0 警告)
- `cargo test --ignore-rust-version`: 全部 56 项测试通过 (30 单元测试 + 26 集成测试全部 passed)
- `./tests/test_backup_restore_e2e.sh`: 全量备份、增量备份、灾难恢复与完整性校验全部通过

## Status
DONE
