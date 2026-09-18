# TASK-015: ClickHouse 系统日志轻量化抑制与写入碎片（Parts）合并防堵调优

## 1. 任务元信息
- **Task ID**: TASK-015
- **父级 Goal**: 系统全面精简与性能飞跃重构（收敛为单一 SQLite + ClickHouse 架构并建设统一备份体系与标准部署体系）
- **状态**: `DONE`
- **依赖任务**: TASK-014 (已完成)
- **创建时间**: 2026-09-18
- **责任人**: AI Agent (Antigravity)

---

## 2. 背景与问题描述
在生产与实际运行中，ClickHouse 作为高通量分析型数据湖引擎，存在两个极其严重的经典隐患：
1. **系统操作日志爆炸（System Logs Explosion）**：
   ClickHouse 默认开启 `system.query_log`、`system.part_log`、`system.trace_log`、`system.text_log` 等表且默认保留期长。在区块链数据高频写入下，每次微批 INSERT 会生成数十倍于数据本身的系统日志与碎片审计，几天内日志体积即可达到几十上百 GB，远超实际业务数据体积，极易撑爆磁盘。
2. **碎片生成快于 Merge 速度导致系统卡死（Too Many Parts / 写入反压）**：
   ClickHouse MergeTree 每次 INSERT 生成一个独立 Part 并由后台 Merge 异步合并。当客户端写入过密且批次较小时，Part 累积速度超过 Merge 速度。一旦达到 `parts_to_delay_insert`（默认 150），ClickHouse 强制休眠延迟写入（导致采集管道严重拖慢）；一旦达到 `parts_to_throw_insert`（默认 300），ClickHouse 直接抛出异常中断写入，导致采集 Worker 挂起假死或陷入失败重试死循环。

---

## 3. 目标与解决方案
实施多层次（四层防御）的系统日志抑制与碎片合并防堵调优体系：
1. **第一层：DDL 表级参数提升容忍度 (`clickhouse/schema.sql`)**：
   在 `raw_logs`、`blocks`、`transactions` 表的 `SETTINGS` 中增加 `parts_to_delay_insert = 300`, `parts_to_throw_insert = 600`, `max_delay_to_insert = 1`。
2. **第二层：客户端连接级异步聚合 (`src/clickhouse/mod.rs`)**：
   在 Rust Client 连接选项中注入 `async_insert = 1` 与 `wait_for_async_insert = 1`，使 ClickHouse 服务端在内存中对微批自动聚合（200ms 或 10MB）再刷入单一 Part，彻底根除碎片累积，且保持 Checkpoint 确认机制的一致性；同时设置 `log_queries = 0` 杜绝采集微批污染 `query_log`。
3. **第三层：服务端配置挂载 (`clickhouse/config.d/` 与 `clickhouse/users.d/`)**：
   - `config.d/system_logs.xml`：将 `query_log`、`text_log`、`metric_log` 的 TTL 缩短为 1~2 天，彻底移除高开销的 `trace_log`，调大刷盘间隔并调大后台 `background_pool_size`。
   - `users.d/tuning.xml`：为 `default` 配置文件启用 `async_insert` 与默认免记录规则。
   - 在 `docker-compose.yml` 与 `docker-compose.source.yml` 中挂载上述配置。
4. **第四层：AI 事实来源固化 (`docs/AI/`)**：
   在 `docs/AI/ARCHITECTURE.md` 与 `docs/AI/DECISIONS.md`（新增 ADR-008）中永久记录该准则，确保后续 AI 与开发者严格遵守。

---

## 4. 涉及文件清单
- `clickhouse/config.d/system_logs.xml` (新建)
- `clickhouse/users.d/tuning.xml` (新建)
- `clickhouse/schema.sql` (修改)
- `src/clickhouse/mod.rs` (修改)
- `src/configuration/mod.rs` (修改)
- `docker-compose.yml` (修改)
- `docker-compose.source.yml` (修改)
- `docs/AI/DECISIONS.md` (修改)
- `docs/AI/ARCHITECTURE.md` (修改)
- `docs/AI/TASK_INDEX.md` (修改)
- `docs/AI/SESSION_STATE.md` (修改)

---

## 5. 验收标准与验证结果 (Acceptance Criteria)
1. [x] ClickHouse 自定义 XML 配置符合官方语法，`docker-compose.yml` 与 `docker-compose.source.yml` 挂载合法并通过 `docker compose config` 验证。
2. [x] `clickhouse/schema.sql` 增加了针对 `raw_logs`、`blocks` 和 `transactions` 的 `parts_to_delay_insert`、`parts_to_throw_insert` 与 `max_delay_to_insert` 表级配置。
3. [x] `src/clickhouse/mod.rs` 实现了 `async_insert=1`、`wait_for_async_insert=1`、`async_insert_busy_timeout_ms=200` 与 `log_queries=0` 动态注入，`src/configuration/mod.rs` 支持环境变量配置。
4. [x] `cargo check --ignore-rust-version --all-targets` 编译通过。
5. [x] `cargo test --ignore-rust-version` 全部 55 个单元与集成测试用例通过（0 failures）。
6. [x] `docs/AI/ARCHITECTURE.md` 与 `docs/AI/DECISIONS.md` 包含完整的架构准则与 ADR-008 记录。

