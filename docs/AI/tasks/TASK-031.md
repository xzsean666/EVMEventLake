# TASK-031: 优化容器部署网络安全与数据持久化 (收敛 ClickHouse 暴露仅保留 API 端口，支持自定义 CLICKHOUSE_DATA_DIR)

## Objective
优化 Docker Compose 与部署脚本，将 ClickHouse 端口（8123/9000）收敛至容器内部虚拟网络，宿主机仅向外暴露 EventLake API 端口；同时在 Compose 与环境变量中支持用户自定义 ClickHouse 数据持久化目录（`CLICKHOUSE_DATA_DIR`），未配置时默认回退至 `./data/clickhouse`。

## Scope
- 包含：
  1. 修改 `docker-compose.yml` 与 `docker-compose.source.yml`，移除 ClickHouse 的宿主机端口映射（`ports` 改为 `expose`），EventLake API 保持唯一宿主机暴露端口。
  2. 修改 `docker-compose.yml` 与 `docker-compose.source.yml` 中 ClickHouse 的数据卷挂载为 `${CLICKHOUSE_DATA_DIR:-./data/clickhouse}:/var/lib/clickhouse`。
  3. 在 `.env.example` 与 `.env.dev` 中补充 `CLICKHOUSE_DATA_DIR` 的配置说明与注释。
  4. 优化 `scripts/deploy-remote.sh`，支持远端按需自动创建用户配置的 `CLICKHOUSE_DATA_DIR` 目录，并更新部署成功提示（强调 ClickHouse 处于容器私有网络）。
  5. 优化 `scripts/backup.sh` 与 `scripts/restore.sh`，增加 ClickHouse 端口未暴露时自动尝试通过 Docker 容器执行 SQL 的兼容支持。
  6. 更新部署文档 `docs/DEPLOYMENT.md`。
- 不包含：
  - 不修改 Rust 核心业务与数据库操作代码。
  - 不修改用户已有的 `config/soneium_blocks_transactions.json`。

## Allowed Files
- `docker-compose.yml`
- `docker-compose.source.yml`
- `.env.example`
- `.env.dev`
- `scripts/deploy-remote.sh`
- `scripts/backup.sh`
- `scripts/restore.sh`
- `docs/DEPLOYMENT.md`
- `docs/AI/tasks/TASK-031.md`
- `docs/AI/TASK_INDEX.md`
- `docs/AI/SESSION_STATE.md`

## Dependencies
- 前置任务：TASK-020 (远程一键部署自动化脚本)
- 外部依赖：Docker Compose V2 / V1

## Inputs and Outputs
- **Inputs**: 环境变量 `CLICKHOUSE_DATA_DIR`（可选），`EVENTLAKE_HTTP_PORT`
- **Outputs**: 安全强化的 `docker-compose.yml`、更新的配置文件模板与自动化脚本

## Acceptance Criteria
- [x] 标准 1：`docker compose config` 验证 ClickHouse 服务无 `ports` 宿主机绑定，仅声明 `expose: [8123, 9000]`；整个系统仅 `eventlake` 服务声明 `ports` 映射。
- [x] 标准 2：当未指定 `CLICKHOUSE_DATA_DIR` 时，挂载路径正确回退为 `./data/clickhouse:/var/lib/clickhouse`；当指定自定义路径时，正确挂载指定路径。
- [x] 标准 3：`.env.example` 与 `.env.dev` 中包含规范的配置项说明。
- [x] 标准 4：`scripts/deploy-remote.sh` 能够识别并初始化自定义的 `CLICKHOUSE_DATA_DIR`，且成功摘要提示准确。
- [x] 标准 5：全库静态检查与所有既有测试保持无破坏。

## Verification Commands
```bash
# 验证 Docker Compose 语法与端口/卷渲染
docker compose config
CLICKHOUSE_DATA_DIR=/tmp/custom_ch docker compose config

# 验证源码构建 Compose 文件
docker compose -f docker-compose.source.yml config

# 验证语法与测试
cargo check
```

## Risks and Assumptions
- 风险：若用户此前依赖在宿主机通过外部工具（如 DBeaver / ClickHouse Client）直连 8123 查询，端口不暴露后需通过 `docker exec` 或 SSH 隧道进行访问。
- 假设：Docker 内部 bridge 网络下，`eventlake` 容器能正常通过 `http://clickhouse:8123` 与 ClickHouse 容器通信。

## Status
DONE
