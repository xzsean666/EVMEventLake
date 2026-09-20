# TASK-020: 远程一键部署自动化脚本 (deploy-remote.sh) 与部署文档支持

## Objective
提供一套工业级、健壮的远程一键部署脚本 `scripts/deploy-remote.sh`，支持用户通过命令行参数或交互式输入指定 SSH 目标主机、远端部署目录及指定的本地 `.env` 配置文件，实现项目文件极简增量同步、容器编排构建启动、健康检查验证及现有持久化数据保护。

## Scope
- 包含：
  - 新增 `scripts/deploy-remote.sh` 脚本，支持 CLI 参数解析（`-s/--ssh`, `-d/--dir`, `-e/--env`, `-p/--port`, `-i/--identity`, `--cn` 等）与缺失参数时的交互式引导。
  - 前置环境校验（本地依赖 `ssh`、`rsync`/`tar`，指定的 `.env` 存在性，远端 Docker 与 Docker Compose 连通与可用性检测）。
  - 安全过滤与高效同步（自动排除 `.git`、`target`、`data`、`logs`、`backups`、本地测试缓存，优先同步 `deploy/prebuilt` 秒级打包）。
  - 自动创建远端宿主机持久化目录（`data/sqlite`、`data/clickhouse`、`logs/clickhouse`、`backups`），保证已有数据不被破坏。
  - 远端构建并拉起 Docker Compose 服务，进行健康检查轮询与状态报告。
  - 更新 `docs/DEPLOYMENT.md`，增加远程一键部署使用说明。
- 不包含：
  - 侵入式修改现有后端 Rust 业务代码或 ClickHouse DDL。

## Allowed Files
- `scripts/deploy-remote.sh`
- `docs/DEPLOYMENT.md`
- `docs/AI/tasks/TASK-020.md`
- `docs/AI/TASK_INDEX.md`
- `docs/AI/SESSION_STATE.md`

## Dependencies
- 前置任务：None
- 外部依赖：本地 bash/ssh，远端 Docker 与 Compose 运行环境

## Inputs and Outputs
- **Inputs**:
  - SSH 连接目标 (如 `user@host` 或通过 `-p` 端口、`-i` 密钥文件)
  - 远端目标目录 (如 `/opt/eventlake`)
  - 本地指定 `.env` 文件路径 (如 `.env.production`)
  - 网络加速开关 (可选 `--cn`)
- **Outputs**:
  - 可执行部署脚本 `scripts/deploy-remote.sh`
  - 远端服务器服务拉起、挂载持久化目录建立、HTTP 健康端点就绪响应

## Acceptance Criteria
- [x] 脚本参数解析支持完整，且当未传参时提供友好的交互式命令行引导。
- [x] 严格校验本地 `.env` 文件的存在性，并能安全推送至远端 `$REMOTE_DIR/.env`（设置 600 权限）。
- [x] 远端持久化目录（`data/sqlite`、`data/clickhouse`）得到安全创建且在重部署时不被清理。
- [x] 同步机制安全健壮，自动剔除大体积无关目录（`.git`, `target`, `data`, `logs` 等）。
- [x] 脚本语法检查通过 (`bash -n scripts/deploy-remote.sh`) 且各分支逻辑严密。
- [x] 文档 `docs/DEPLOYMENT.md` 包含清晰的使用范例。

## Verification Commands
```bash
# 1. 语法正确性检查
bash -n scripts/deploy-remote.sh

# 2. 帮助信息输出测试
./scripts/deploy-remote.sh --help

# 3. 基础参数缺失与错误处理测试
./scripts/deploy-remote.sh -s root@127.0.0.1 -d /opt/eventlake -e non_existent_env.env
./scripts/deploy-remote.sh -s invalid.test.host.local -d /opt/eventlake -e .env.example --dry-run
```

## Verification Results
- 语法与格式：`bash -n scripts/deploy-remote.sh` 退出码 0。
- 帮助信息：`./scripts/deploy-remote.sh --help` 正常展示各参数与详细示例。
- 容错验证：当指定不存在的 env 文件时阻断报错；当指定非法 SSH 域名时报错退出。
- 全量构建验证：`cargo check --ignore-rust-version --all-targets` 正常通过。

## Risks and Assumptions
- 风险：若远端未安装 Docker 或权限不足（非 root 且未加入 docker 组），脚本已内置环境嗅探并给出友好指引。
- 假设：本地具备 SSH 客户端与网络访问权限。

## Status
DONE
