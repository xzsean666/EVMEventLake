# TASK-011: 清理冗余 Dockerfile 与 Compose 配置，统一构建与部署流水线至纯净 SQLite + ClickHouse

## Objective
移除历史过渡期遗留的 6 个冗余 Docker/Compose 文件（`*.clickhouse*`），统一并优化保留的 3 套标准部署文件（源码构建、预编译构建、国内镜像预编译），清理关联构建脚本与过时文档中的 PostgreSQL / 废弃 Docker 配置说明，使仓库配置与架构完全收敛至单一的「SQLite 控制面 + ClickHouse 数据湖」。

## Scope
- **包含**：
  - 删除冗余 Docker/Compose 文件：
    - `Dockerfile.clickhouse`
    - `Dockerfile.prebuilt.clickhouse`
    - `Dockerfile.prebuilt.clickhouse.cn`
    - `docker-compose.clickhouse.yml`
    - `docker-compose.prebuilt.clickhouse.yml`
    - `docker-compose.prebuilt.clickhouse.cn.yml`
  - 更新并优化保留的三套核心配置：
    - `docker-compose.yml`: 移除废弃 postgres 服务与 volume，对齐统一的 SQLite (`./data/sqlite:/data`) + ClickHouse (`./data/clickhouse`) 架构。
    - `Dockerfile`: 包含 `COPY clickhouse ./clickhouse` 确保 `include_str!` 编译通过。
    - `Dockerfile.prebuilt` / `docker-compose.prebuilt.yml`: 保证指向唯一标准产物 `deploy/prebuilt/eventlake`。
    - `Dockerfile.prebuilt.cn` / `docker-compose.prebuilt.cn.yml`: 保证国内镜像源配置正确并统一指向 `deploy/prebuilt/eventlake`。
  - 清理构建脚本与辅助说明：
    - `scripts/build-prebuilt-binary.sh`: 移除已失效的 `eventlake-clickhouse` 分支与 `--features clickhouse`。
    - `deploy/prebuilt/README.md`: 移除 `eventlake-clickhouse` 与过时 Compose 变体描述。
  - 文档对齐与残留注释修正：
    - `docs/DEPLOYMENT.md`: 更新部署矩阵为仅 3 套模式，清理删除文件的说明。
    - `docs/USAGE.md`: 移除废弃 Compose 指令和过时说明。
    - `docs/BUILD.md`: 移除 Postgres 依赖与旧命令说明。
    - `docs/AI/ARCHITECTURE.md`: 架构说明收敛为单一 SQLite + ClickHouse，移除旧双存储模式描述。
    - `README.md`: 快速开始与系统介绍同步。
    - `src/clickhouse/mod.rs`: 清理历史残留的 PostgreSQL 注释与报错文案。
- **不包含**：
  - 不修改 Rust 核心业务逻辑与数据流。
  - 不修改 SQLite 迁移文件与备份脚本。

## Allowed Files
- `docker-compose.yml`
- `Dockerfile`
- `scripts/build-prebuilt-binary.sh`
- `deploy/prebuilt/README.md`
- `src/clickhouse/mod.rs`
- `docs/DEPLOYMENT.md`
- `docs/USAGE.md`
- `docs/BUILD.md`
- `docs/AI/ARCHITECTURE.md`
- `README.md`
- `docs/AI/tasks/TASK-011.md`
- `docs/AI/TASK_INDEX.md`
- `docs/AI/SESSION_STATE.md`

## Dependencies
- 前置任务：TASK-009, TASK-010
- 外部依赖：None

## Inputs and Outputs
- **Inputs**: 现存各类 Dockerfile、docker-compose*.yml、部署文档与构建脚本。
- **Outputs**: 清爽统一的 3 套 Docker 镜像与 Compose 配置体系，对齐最新的纯 SQLite + ClickHouse 架构文档。

## Acceptance Criteria
- [x] 标准 1：6 个 `*.clickhouse*` 冗余 Docker/Compose 文件被物理删除。
- [x] 标准 2：`docker-compose.yml` 完全重构为 SQLite + ClickHouse 服务组合，无任何 postgres 容器或卷残留。
- [x] 标准 3：三套标准 Compose 文件（`docker-compose.yml`、`docker-compose.prebuilt.yml`、`docker-compose.prebuilt.cn.yml`）均通过 `docker compose config` 语法验证。
- [x] 标准 4：`Dockerfile` 补充 `COPY clickhouse ./clickhouse` 目录复制，保证完整上下文构建。
- [x] 标准 5：`scripts/build-prebuilt-binary.sh` 与 `deploy/prebuilt/README.md` 清理完成，统一产物路径。
- [x] 标准 6：`docs/DEPLOYMENT.md`、`docs/USAGE.md`、`docs/BUILD.md`、`docs/AI/ARCHITECTURE.md` 及 `README.md` 全量同步为单一存储架构。
- [x] 标准 7：`cargo check --ignore-rust-version --all-targets` 与全部单元/集成测试 `cargo test --ignore-rust-version` 运行 100% 通过。

## Verification Commands
```bash
# 1. 验证冗余文件已清除
! ls Dockerfile*.clickhouse* docker-compose*.clickhouse* 2>/dev/null

# 2. 验证保留的 3 个 Compose 文件语法
docker compose --env-file .env.example -f docker-compose.yml config > /dev/null
docker compose --env-file .env.example -f docker-compose.prebuilt.yml config > /dev/null
docker compose --env-file .env.example -f docker-compose.prebuilt.cn.yml config > /dev/null

# 3. 验证构建脚本
scripts/build-prebuilt-binary.sh

# 4. 验证 Rust 静态检查与全部测试
cargo check --ignore-rust-version --all-targets
cargo test --ignore-rust-version
```

## Risks and Assumptions
- 风险：若有用户脚本硬编码使用了旧 compose 文件名需做变更，但统一配置可大幅降低后续维护混乱
- 假设：当前环境下 Dockerfile 和 3 个标准 compose 文件已完全覆盖日常与 CI/CD 场景

## Status
DONE
