# 会话状态记录 (SESSION STATE)

本文档记录当前开发会话的状态，是跨 Session 恢复工作的直接依据。

---

## 1. 核心状态概要

- **当前 Goal**: 系统全面精简与性能飞跃重构（收敛为单一 SQLite + ClickHouse 架构并建设统一备份体系与标准部署体系）
- **当前 Task**: 
  - **TASK-009**: 存储层全面迁移至 SQLite 元数据引擎 + ClickHouse 唯一数据湖 (`DONE`)
  - **TASK-010**: 构建 SQLite + ClickHouse 统一备份恢复体系与 S3 增量运维脚本 (`DONE`)
  - **TASK-011**: 清理冗余 Dockerfile 与 Compose 配置，统一构建与部署流水线至纯净 SQLite + ClickHouse (`DONE`)
- **当前状态**: `DONE` (所有冗余文件清理完毕，保留的 3 套标准 Compose 语法与构建测试 100% 验证通过)

---

## 2. 本次会话完成内容 (Accomplished Work)

### 2.1 彻底剥离 PostgreSQL 依赖，收敛为 SQLite + ClickHouse 架构 (TASK-009)
1. **依赖体系重构 (`Cargo.toml`)**:
   - `sqlx`: 将 `postgres` feature 替换为 `sqlite`，保留 `runtime-tokio`, `tls-rustls`, `uuid`, `chrono`, `json`, `migrate`, `macros`。
   - `clickhouse` 与 `time`: 提升为系统核心强依赖，消除 optional feature 编译分支与冗余的条件编译宏。
2. **纯 SQLite 基线数据库迁移 (`migrations/202609180001_initial_schema.sql`)**:
   - 彻底删除 PostgreSQL 专有原始日志表 `eventlake_raw_logs` 及其默认分区与索引。
   - 保留控制面 6 张核心表（`chains`, `rpc_endpoints`, `subscriptions`, `block_checkpoints`, `api_keys`, `block_transaction_sync_state`）。
   - 将所有 PG 方言函数与类型对齐 SQLite（如 `now()` -> `CURRENT_TIMESTAMP`，`UUID`/`JSONB` -> `TEXT`，`BOOLEAN` -> `INTEGER 0/1`）。
3. **Rust 业务代码层全面改造**:
   - `src/database/mod.rs` & `src/app/application_state.rs`: 连接池切换为 `SqlitePool`，启用 WAL 模式 (`PRAGMA journal_mode = WAL;`) 与 5s 繁忙超时。
   - 业务模块（`chains`, `rpc_pool`, `subscriptions`, `reorg`, `auth`, `block_transaction`, `dashboard`）全面切换为 `SqlitePool`。
   - `src/search/mod.rs`: 移除了针对 PostgreSQL 的原始日志兜底查询逻辑（`execute_postgres_raw_log_search` 及 `QueryBuilder<sqlx::Postgres>` 过滤/排序器），无条件直连 ClickHouse。
   - `src/collector/worker.rs`: 移除分区管理器依赖，原始事件日志直写 ClickHouse。
   - 彻底移除废弃的 `src/indexing/partition_manager.rs` 与 `src/indexing/mod.rs`。
4. **集成测试套件纯 SQLite 化**:
   - `tests/e2e_real_database_tests.rs`: 切换至 `SqlitePool`，默认自动回退至临时本地 SQLite 文件，移除 `eventlake_raw_logs` 表断言，改为测试核心元数据流。
   - `tests/rpc_pool_cooldown_test.rs`, `tests/clickhouse_integration_tests.rs`, `tests/live_real_evm_data_tests.rs`: 全量更新为 `SqlitePoolOptions` 与 `sqlite::memory:`。

### 2.2 统一备份恢复体系与 S3 增量运维脚本 (TASK-010)
1. **一键备份脚本 (`scripts/backup.sh`)**:
   - 支持 `--full`（全量）、`--incremental`（增量）、`--local`（本地归档）与 `--s3`（云端直推）。
   - SQLite 使用原生 `VACUUM INTO` 实现运行期零停机、无锁冲突的原子快照与碎片整理。
   - ClickHouse 使用原生 `BACKUP DATABASE eventlake TO ... [SETTINGS base_backup = ...]` 实现文件/S3 增量分片导出。
   - 自动生成 SHA256 签名与 `manifest.json` 元数据清单；本地自动清理超过 `BACKUP_RETENTION_DAYS` 的过期备份。
2. **一键灾难恢复脚本 (`scripts/restore.sh`)**:
   - 支持一键从本地备份目录或 S3 云端拉取指定备份并恢复。
   - 自动备份现有运行中 SQLite 数据库为 `.bak.<timestamp>` 防错。
   - 执行 `PRAGMA integrity_check` 保证恢复后的 SQLite 数据库 100% 结构健康。
   - 触发 ClickHouse `RESTORE DATABASE eventlake FROM ...` 恢复数据面。
3. **完整性校验工具 (`scripts/verify-backup.sh`)**:
   - 读取备份清单，校验 SQLite 文件哈希签名，执行完整性自检并打印表级数据行数统计。
4. **零依赖 S3 传输工具 (`scripts/s3-helper.py`)**:
   - 基于 Python 3 原生标准库实现 AWS SigV4 协议，无需安装 `aws-cli` 或 `boto3`，开箱即用支持 AWS S3, Cloudflare R2, MinIO, 阿里云 OSS 等对象存储。
5. **端到端自动化测试 (`tests/test_backup_restore_e2e.sh`)**:
   - 自动验证“初始化数据 -> 全量备份 -> 新增数据 -> 增量备份 -> 模拟灾难删库 -> 恢复增量备份 (7条数据) -> 恢复全量备份 (6条数据)”完整闭环，全部通过！
6. **详尽运维指南与容器更新**:
   - 编写 [`docs/BACKUP_AND_RESTORE.md`](file:///ssd0/git/EVMEventLake/docs/BACKUP_AND_RESTORE.md)。

### 2.3 清理冗余配置与收敛标准部署流水线 (TASK-011)
1. **删除 6 个历史过度期冗余 Docker/Compose 文件**:
   - 删除了 `Dockerfile.clickhouse`、`Dockerfile.prebuilt.clickhouse`、`Dockerfile.prebuilt.clickhouse.cn`
   - 删除了 `docker-compose.clickhouse.yml`、`docker-compose.prebuilt.clickhouse.yml`、`docker-compose.prebuilt.clickhouse.cn.yml`
2. **优化保留的三套标准容器配置体系**:
   - `docker-compose.yml`: 重写为纯净 SQLite + ClickHouse，移除旧 `postgres` 容器及 volume。
   - `docker-compose.prebuilt.yml`: 统一指向 `deploy/prebuilt/eventlake`。
   - `docker-compose.prebuilt.cn.yml`: 保留国内源镜像代理并统一指向 `deploy/prebuilt/eventlake`。
   - `Dockerfile`: 增加 `COPY clickhouse ./clickhouse` 解决编译期内嵌 `clickhouse/schema.sql` 依赖。
3. **清理构建脚本与辅助说明**:
   - `scripts/build-prebuilt-binary.sh`: 移除 `--features clickhouse` 传参分支，增加 `--ignore-rust-version` 保证平滑跨版本构建。
   - `deploy/prebuilt/README.md`: 移除过时的 `eventlake-clickhouse` 描述。
4. **全量修正架构与运维文档**:
   - `docs/DEPLOYMENT.md`: 收敛为 3 种标准模式，清理旧 Compose 变体。
   - `docs/USAGE.md`: 移除旧命令与旧 PG 说明，更新为统一数据湖流程。
   - `docs/BUILD.md`: 全面更新为 2.0 版本规范说明。
   - `docs/AI/ARCHITECTURE.md`: 确立 SQLite + ClickHouse 纯净分层单一架构。
   - `README.md`: 同步项目愿景、定位与快速开始指令。
   - `src/clickhouse/mod.rs`: 清理代码注释与报错中历史残留的 PostgreSQL 字样。

---

## 3. 文件变动清单

### 新建文件 (Created Files)
- `scripts/backup.sh`: 统一 SQLite + ClickHouse 全量/增量备份脚本。
- `scripts/restore.sh`: 统一灾难恢复脚本。
- `scripts/verify-backup.sh`: 备份包完整性校验工具。
- `scripts/s3-helper.py`: 零依赖 S3 / MinIO / Cloudflare R2 上传下载工具。
- `tests/test_backup_restore_e2e.sh`: 备份与恢复全流程自动化回归测试。
- `docs/BACKUP_AND_RESTORE.md`: 统一备份恢复与灾难演练运维手册。
- `docs/AI/tasks/TASK-009.md`: TASK-009 任务定义与完成记录。
- `docs/AI/tasks/TASK-010.md`: TASK-010 任务定义与完成记录。
- `docs/AI/tasks/TASK-011.md`: TASK-011 任务定义与完成记录。

### 删除文件 (Deleted Files)
- `Dockerfile.clickhouse`
- `Dockerfile.prebuilt.clickhouse`
- `Dockerfile.prebuilt.clickhouse.cn`
- `docker-compose.clickhouse.yml`
- `docker-compose.prebuilt.clickhouse.yml`
- `docker-compose.prebuilt.clickhouse.cn.yml`
- `src/indexing/mod.rs`
- `src/indexing/partition_manager.rs`

### 调整修正的文件 (Modified Files)
- `docker-compose.yml`: 重写为纯净 SQLite + ClickHouse。
- `docker-compose.prebuilt.yml` & `docker-compose.prebuilt.cn.yml`: 统一指向 `deploy/prebuilt/eventlake`。
- `Dockerfile`: 补齐 `COPY clickhouse ./clickhouse` 目录复制。
- `scripts/build-prebuilt-binary.sh`: 清理 clickhouse feature 传参分支。
- `deploy/prebuilt/README.md`: 清理废弃二进制变体说明。
- `src/clickhouse/mod.rs`: 清理残留 PostgreSQL 注释与报错。
- `README.md`: 快速开始与架构描述更新。
- `docs/DEPLOYMENT.md` & `docs/USAGE.md`: 部署与使用手册同步更新。
- `docs/BUILD.md`: 构建指南同步更新。
- `docs/AI/ARCHITECTURE.md`: 权威架构说明同步更新。
- `docs/AI/TASK_INDEX.md`: 看板更新 TASK-011 为 DONE。

---

## 4. 已运行的验证命令及结果

```bash
# 1. 3 套标准 Compose 文件语法校验 (全部通过)
docker compose --env-file .env.example -f docker-compose.yml config > /dev/null
docker compose --env-file .env.example -f docker-compose.prebuilt.yml config > /dev/null
docker compose --env-file .env.example -f docker-compose.prebuilt.cn.yml config > /dev/null

# 2. 预编译二进制构建脚本运行 (成功生成 deploy/prebuilt/eventlake, 19MB)
./scripts/build-prebuilt-binary.sh

# 3. 全目标静态编译分析 (0 error, 0 warning)
cargo check --ignore-rust-version --all-targets

# 4. 单元与集成测试套件全部通过 (55 passed; 0 failed)
cargo test --ignore-rust-version

# 5. 备份与恢复端到端闭环测试全部通过 (9 步验证均 OK)
./tests/test_backup_restore_e2e.sh
```

---

## 5. 未解决问题 (Known Issues)

- 无。

---

## 6. 风险和假设 (Risks and Assumptions)

- **假设**: 用户若有现有脚本硬编码使用了 `docker-compose.clickhouse.yml` 等文件名，应调整为直接使用标准的 `docker-compose.yml` 或 `docker-compose.prebuilt.yml`。
- **风险**: 无。系统代码与部署形态已经高度精炼与收敛。

---

## 7. 下一步计划 (Next Task)

- **下一步建议**: 所有阶段性架构精简与备份恢复流水线（TASK-009、TASK-010、TASK-011）已圆满闭环，系统架构完全收敛至「SQLite 控制面 + ClickHouse 数据湖 + S3 统一增量备份」。
- **下一次 Session 应先读取的文件**:
  1. [`AGENTS.md`](file:///ssd0/git/EVMEventLake/AGENTS.md)
  2. [`docs/AI/GOAL.md`](file:///ssd0/git/EVMEventLake/docs/AI/GOAL.md)
  3. [`docs/AI/TASK_INDEX.md`](file:///ssd0/git/EVMEventLake/docs/AI/TASK_INDEX.md)
  4. [`docs/AI/SESSION_STATE.md`](file:///ssd0/git/EVMEventLake/docs/AI/SESSION_STATE.md)
