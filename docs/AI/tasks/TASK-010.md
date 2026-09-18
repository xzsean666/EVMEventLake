# TASK-010: 构建 SQLite + ClickHouse 统一备份恢复体系与 S3 增量运维脚本

## Objective
在 TASK-009 确立的 SQLite + ClickHouse 单一数据湖架构基础上，构建一套高可靠、低开销的统一备份与恢复工具链。实现 SQLite 元数据快照与 ClickHouse 原始日志不可变分片（Parts）的联动；提供开箱即用、交互友好的 Shell 运维脚本（支持本地、AWS S3、Cloudflare R2 与 MinIO），实现全量备份、定时增量备份以及一键灾难恢复。

## Scope
- **包含**：
  1. **备份与恢复核心逻辑**：
     - SQLite 在线原子快照机制（利用 `VACUUM INTO` 保证运行期零损坏且极轻量）。
     - ClickHouse 原生 S3 增量备份命令（基于 `BACKUP ... TO S3(...) SETTINGS base_backup = ...`）或专用工具集成。
     - 元数据清单记录（`backup_meta.json`，记录时间戳、备份类型、区块高度 Checkpoints、base_backup 链条）。
  2. **一键运维 Shell 脚本工具集 (`scripts/`)**：
     - `scripts/backup.sh`：支持 `--full`（全量）、`--incremental`（增量）、`--s3`（云端直推）、`--local`（本地归档）。
     - `scripts/restore.sh`：支持一键从本地或 S3 拉取并恢复 SQLite 元数据与 ClickHouse 表，实现无缝自愈。
     - `scripts/verify-backup.sh`：校验备份包完整性与元数据有效性。
  3. **环境与配置模板更新**：
     - 在 `.env.example` 中补充对象存储配置（`BACKUP_S3_ENDPOINT`, `BACKUP_S3_BUCKET`, `BACKUP_S3_ACCESS_KEY`, `BACKUP_S3_SECRET_KEY` 等）。
  4. **运维与使用文档更新**：
     - 编写详细操作手册（`docs/BACKUP_AND_RESTORE.md`），提供定时 Cron 任务配置示例与容灾演练指引。
- **不包含**：
  1. 不修改采集业务核心数据流。

## Allowed Files
- `scripts/*`
- `.env.example`
- `docs/BACKUP_AND_RESTORE.md`
- `docs/DEPLOYMENT.md`
- `docs/USAGE.md`
- `docs/AI/TASK_INDEX.md`
- `docs/AI/tasks/TASK-010.md`
- `docs/AI/SESSION_STATE.md`

## Dependencies
- 前置任务：TASK-009 (待完成)
- 架构决策：ADR-007

## Inputs and Outputs
- **Inputs**: SQLite + ClickHouse 运行环境与 ADR-007 设计方案。
- **Outputs**: 完整的备份恢复脚本套件、S3 增量支持、自动化校验与详细运维指南。

## Acceptance Criteria
- [x] 1. 提供清晰友好的 `scripts/backup.sh` 脚本，支持 `--full` 与 `--incremental` 模式并可推送到 S3。
- [x] 2. 提供 `scripts/restore.sh` 脚本，能从指定全量或增量备份点一键还原 SQLite 与 ClickHouse。
- [x] 3. 编写端到端验证测试，验证“写入数据 -> 全量备份 -> 新写入 -> 增量备份 -> 灾难恢复”整个闭环。
- [x] 4. 交付完整的文档与 CronJob 配置样例。

## Verification Commands
```bash
./scripts/backup.sh --local --full
./scripts/backup.sh --local --incremental
./scripts/restore.sh --local --target <backup_name>
./tests/test_backup_restore_e2e.sh
```

## Risks and Assumptions
- **假设**：目标环境具备 `sqlite3`、`curl` 或 ClickHouse 客户端工具，以及 S3 兼容的对象存储凭据。
- **风险**：ClickHouse 在执行大表备份时对 I/O 的占用，通过设置带宽限速或低峰期执行规避。

## Status
DONE
