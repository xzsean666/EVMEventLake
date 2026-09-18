# EVMEventLake 统一备份与灾难恢复运维指南 (Backup & Disaster Recovery)

本文档介绍 EVMEventLake 在 **SQLite (控制面元数据) + ClickHouse (唯一原始日志与区块交易湖)** 架构下的统一备份、增量同步与一键灾难恢复工具链。

---

## 1. 架构与备份原理

EVMEventLake 采用双存储协同模式：
1. **控制面元数据 (SQLite)**: 存储 6 张核心控制表（`chains`, `rpc_endpoints`, `subscriptions`, `block_checkpoints`, `api_keys`, `block_transaction_sync_state`）。
   - **备份技术**: 采用 SQLite 原生 `VACUUM INTO` 在线原子快照机制。运行期零停机、无写锁冲突，并将数据库自动整理紧凑为单一纯净文件，生成 SHA-256 校验和。
2. **数据面事件湖 (ClickHouse)**: 存储海量不可变区块、交易与原始日志（`raw_logs`, `blocks`, `transactions`）。
   - **备份技术**: 采用 ClickHouse 原生 SQL `BACKUP DATABASE eventlake TO ... [SETTINGS base_backup = ...]`。全量时全量打硬链接/分片，增量时仅记录自上一次基线以来新合并/写入的 Parts，支持直接推送至 S3/MinIO/Cloudflare R2。
3. **元数据清单 (`manifest.json`)**: 记录每次备份的唯一 Tag、时间戳、模式（full / incremental）、基线关联链条、文件 SHA256 签名及大小。

---

## 2. 核心运维脚本概览

所有备份与恢复脚本均位于项目根目录的 [`scripts/`](file:///ssd0/git/EVMEventLake/scripts/) 下，具有零第三方复杂依赖（仅依赖系统内置的 `python3` 与 `curl`）：

| 脚本 | 功能定位 | 主要参数 |
| :--- | :--- | :--- |
| [`scripts/backup.sh`](file:///ssd0/git/EVMEventLake/scripts/backup.sh) | 统一备份入口 | `--full` (全量), `--incremental` (增量), `--local` (本地), `--s3` (云端直传) |
| [`scripts/restore.sh`](file:///ssd0/git/EVMEventLake/scripts/restore.sh) | 一键灾难恢复 | `--target <tag_or_dir>`, `--from-s3`, `--local`, `-y/--yes` (跳过确认) |
| [`scripts/verify-backup.sh`](file:///ssd0/git/EVMEventLake/scripts/verify-backup.sh) | 备份完整性自检 | `<backup_tag_or_dir>` (检查 SHA256 与 `PRAGMA integrity_check`) |
| [`scripts/s3-helper.py`](file:///ssd0/git/EVMEventLake/scripts/s3-helper.py) | 零依赖 S3 SigV4 传输 | 支持 AWS S3, Cloudflare R2, MinIO, 阿里云 OSS, 腾讯云 COS |

---

## 3. 环境变量与配置 (`.env`)

在项目根目录的 `.env` 中配置存储与备份路径：

```bash
# ------------------------------------------------------------------------------
# 基础运行与数据库配置
# ------------------------------------------------------------------------------
EVENTLAKE_DATABASE_URL=sqlite://data/eventlake.db?mode=rwc
EVENTLAKE_CLICKHOUSE_ENABLED=true
EVENTLAKE_CLICKHOUSE_URL=http://eventlake:eventlake@clickhouse:8123/eventlake

# ------------------------------------------------------------------------------
# 备份与对象存储配置 (AWS S3 / MinIO / Cloudflare R2)
# ------------------------------------------------------------------------------
BACKUP_DIR=./backups
BACKUP_RETENTION_DAYS=7

# 是否启用 S3 远端同步
BACKUP_S3_ENABLED=true
BACKUP_S3_ENDPOINT=https://s3.us-east-1.amazonaws.com # 如使用 MinIO: http://minio:9000; 如使用 R2: https://<account_id>.r2.cloudflarestorage.com
BACKUP_S3_BUCKET=my-eventlake-backups
BACKUP_S3_REGION=us-east-1
BACKUP_S3_PREFIX=eventlake-prod
BACKUP_S3_ACCESS_KEY=your_access_key
BACKUP_S3_SECRET_KEY=your_secret_key
```

---

## 4. 备份操作实战

### 4.1 本地全量备份 (Local Full Backup)
```bash
./scripts/backup.sh --local --full
```
输出示例：
```text
============================================================
 EVMEventLake Unified Backup Initiated
 Time:       20260918_073523Z
 Mode:       full
 Target:     local
 Backup Tag: backup_20260918_073523Z_full
============================================================
==> [1/3] Creating SQLite operational metadata snapshot...
    SQLite snapshot completed: 80K (SHA256: fd76b7b35e07b62e...)
==> [2/3] Processing ClickHouse raw event lake backup...
    ClickHouse backup command succeeded.
==> [3/3] Finalizing backup manifest...
============================================================
 Backup Completed Successfully!
 Manifest:   ./backups/backup_20260918_073523Z_full/manifest.json
 Tag:        backup_20260918_073523Z_full
 Restore:    ./scripts/restore.sh --target backup_20260918_073523Z_full
============================================================
```

### 4.2 本地增量备份 (Local Incremental Backup)
自动识别前一次基线备份 Tag，ClickHouse 仅增量导出新分片（Parts）：
```bash
./scripts/backup.sh --local --incremental
```

### 4.3 云端 S3 / MinIO 直推备份
```bash
# S3 全量备份
./scripts/backup.sh --s3 --full

# S3 增量备份
./scripts/backup.sh --s3 --incremental
```

### 4.4 校验备份包完整性
```bash
./scripts/verify-backup.sh backup_20260918_073523Z_full
```
输出包含：
- Tag 与时间戳验证
- SQLite 文件哈希签名强校验
- `PRAGMA integrity_check` 零损坏判定
- 备份内部各表数据行数快照清单

---

## 5. 灾难恢复与应急演练 (Disaster Recovery)

### 5.1 从最新本地备份一键恢复
当服务异常、数据误删或需要回退时：
```bash
# 交互式恢复最新备份（自动将当前已有库重命名备份为 .bak.<timestamp>）
./scripts/restore.sh

# 静默一键恢复
./scripts/restore.sh --yes
```

### 5.2 从指定历史时间点/Tag 恢复
```bash
./scripts/restore.sh --target backup_20260918_073523Z_full --yes
```

### 5.3 从 S3 云端异地拉取恢复 (全新节点冷启动灾备)
在全新服务器部署机器上配置好 `.env` 中的 S3 凭据后：
```bash
# 从 S3 拉取指定 Tag 进行恢复
./scripts/restore.sh --from-s3 --target backup_20260918_073523Z_full --yes

# 或者直接拉取 S3 上的最新版本
./scripts/restore.sh --from-s3 --yes
```

---

## 6. 定时任务 (Cron Job) 生产配置建议

生产环境建议配置 **每周/每日全量备份 + 每小时增量备份**，并自动同步至 S3：

```crontab
# 编辑 crontab
crontab -e

# 每天凌晨 02:00 执行一次全量备份并上传至 S3 (保留7天)
0 2 * * * /ssd0/git/EVMEventLake/scripts/backup.sh --s3 --full >> /var/log/eventlake_backup.log 2>&1

# 每小时的第 30 分钟执行一次增量备份并上传至 S3
30 * * * * /ssd0/git/EVMEventLake/scripts/backup.sh --s3 --incremental >> /var/log/eventlake_backup.log 2>&1
```
