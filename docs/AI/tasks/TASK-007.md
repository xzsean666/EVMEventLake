# TASK-007: 移除 SSH_TUNNEL 隧道及容器代理相关实现与配置

## Objective
完全移除项目中的 SSH_TUNNEL 隧道脚本、预构建 Dockerfile 包装逻辑、docker-compose 环境变量配置以及相关文档说明，使容器运行直接调用 `eventlake` 且无任何残留代理环境配置。

## Scope
- 包含：
  - 删除 `scripts/docker-ssh-tunnel-proxy.sh` 脚本。
  - 修改 4 个预构建 Dockerfile (`Dockerfile.prebuilt`, `Dockerfile.prebuilt.cn`, `Dockerfile.prebuilt.clickhouse`, `Dockerfile.prebuilt.clickhouse.cn`)，移除 `openssh-client`、脚本拷贝和 `ENTRYPOINT` 代理包装，直接以 `ENTRYPOINT ["eventlake"]` 启动。
  - 修改 4 个预构建 Compose 文件 (`docker-compose.prebuilt*.yml`)，移除全部 `SSH_TUNNEL_*` 环境变量。
  - 清理 `.env.example` 中已无用的 `SSH_TUNNEL_*` 配置项。
  - 更新 `docs/DEPLOYMENT.md` 与 `docs/USAGE.md`，删除 SSH tunnel 相关章节与描述。
- 不包含：
  - 修改业务采集、解析或 API 代码（`src/` 无相关代理逻辑）。
  - 破坏现有 Compose 基础服务（PostgreSQL、ClickHouse、EventLake 本身启动流程）。

## Allowed Files
- `scripts/docker-ssh-tunnel-proxy.sh` (删除)
- `Dockerfile.prebuilt`
- `Dockerfile.prebuilt.cn`
- `Dockerfile.prebuilt.clickhouse`
- `Dockerfile.prebuilt.clickhouse.cn`
- `docker-compose.prebuilt.yml`
- `docker-compose.prebuilt.cn.yml`
- `docker-compose.prebuilt.clickhouse.yml`
- `docker-compose.prebuilt.clickhouse.cn.yml`
- `.env.example`
- `docs/DEPLOYMENT.md`
- `docs/USAGE.md`
- `docs/AI/TASK_INDEX.md`
- `docs/AI/SESSION_STATE.md`
- `docs/AI/tasks/TASK-007.md`

## Dependencies
- 前置任务：TASK-006 (已完成)
- 外部依赖：None

## Inputs and Outputs
- **Inputs**: 用户需求（移除 SSH_TUNNEL 与 proxy 逻辑）。
- **Outputs**: 干净的 Docker 与 Compose 构建/运行配置，无 SSH_TUNNEL 与 proxy 残留。

## Acceptance Criteria
- [x] `scripts/docker-ssh-tunnel-proxy.sh` 被彻底删除。
- [x] 4 个 prebuilt Dockerfile 不再安装 `openssh-client`，ENTRYPOINT 直接为 `["eventlake"]`。
- [x] 4 个 prebuilt Compose 文件不再引用任何 `SSH_TUNNEL_*` 环境变量。
- [x] `.env.example` 不再包含 `SSH_TUNNEL_*` 配置项。
- [x] 文档不再描述已移除的 SSH tunnel 功能。
- [x] `docker compose config` 验证全部 Compose 模板解析正常。
- [x] 全局搜索 `SSH_TUNNEL` 结果为空（除任务文档记录外）。

## Verification Commands
```bash
# 验证无 SSH_TUNNEL 残留
grep -rn "SSH_TUNNEL" --exclude-dir=docs/AI .

# 验证所有 compose 配置语法有效
docker compose -f docker-compose.prebuilt.yml config > /dev/null
docker compose -f docker-compose.prebuilt.cn.yml config > /dev/null
docker compose -f docker-compose.prebuilt.clickhouse.yml config > /dev/null
docker compose -f docker-compose.prebuilt.clickhouse.cn.yml config > /dev/null

# 验证 Rust 代码未受影响
cargo check --ignore-rust-version --all-targets
```

## Risks and Assumptions
- 风险：若存在依赖容器内部动态 SOCKS5 代理的环境，需在宿主机或网络层提供路由，而非应用容器内管理。
- 假设：当前开发与生产环境已不需要此内置 SSH 隧道。

## Status
DONE
