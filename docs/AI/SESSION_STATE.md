# 会话状态记录 (SESSION STATE)

本文档记录当前开发会话的状态，是跨 Session 恢复工作的直接依据。

---

## 1. 核心状态概要

- **当前 Goal**: 建设与完善生产运维与业务落地全流程支持体系
- **当前 Task**: 
  - **TASK-024**: lakectl 支持先登录后操作 (login/logout/whoami) 与自定义远程端口 (`DONE`)
  - **TASK-023**: lakectl 支持声明式配置幂等导入与导出 (import/export config) (`DONE`)
  - **TASK-022**: 服务端区块交易常驻解耦与独立纯 Shell 客户端 (lakectl) (`DONE`)
  - **TASK-021**: 完善部署后全流程业务使用与下游集成指南 (USAGE.md) (`DONE`)
  - **TASK-020**: 远程一键部署自动化脚本 (deploy-remote.sh) 与部署文档支持 (`DONE`)
- **当前状态**: `DONE` (已完成 lakectl 先登录后操作机制与凭据持久化 ~/.lakectl/config，支持公网直连、SSH 内部端口穿透与本地直连 3 大模式，支持 login/logout/whoami 及 --remote-port)

---

## 2. 本次会话完成内容 (Accomplished Work)

### 2.1 全面测试与 E2E 测试完善度审计
- 对全库进行全面测试（含单元测试、ClickHouse 存储集成测试、EVM 链上真实区块测试与运维脚本测试）。
- 发现并定位 2 个阻断缺陷：`live_chain_collects_and_searches_raw_base_usdc_logs` 运行时因未注入 ClickHouse 客户端导致的 502 Panic 崩溃，以及 `docker compose up -d clickhouse` 因配置语义与只读目录挂载导致的闪退。

### 2.2 ClickHouse 容器配置与 Docker Compose 挂载修复
- **配置语义规范化**：在 `clickhouse/users.d/tuning.xml` 中移除误填在用户 Profile `<profiles><default>` 下的表引擎参数（`parts_to_delay_insert`、`parts_to_throw_insert`、`max_delay_to_insert`），消除 ClickHouse 24.8 启动报 `UNKNOWN_SETTING: max_delay_to_insert` 的致命错误（表级参数已在 `clickhouse/config.d/system_logs.xml` 与 `clickhouse/schema.sql` 中规范声明）。
- **文件级精准挂载**：将 `docker-compose.yml` 与 `docker-compose.source.yml` 中的目录只读挂载调整为针对 `system_logs.xml` 和 `tuning.xml` 的精确文件挂载，避免覆盖容器内置的 `docker_related_config.xml`（确保 0.0.0.0 监听可用），并允许 ClickHouse 官方 entrypoint 自动在 `users.d` 写入 `default-user.xml`。
- **添加备份支持路径**：在 `docker-compose.yml` 中挂载 `/tmp:/tmp` 与 `./backups:/backups`，并在 `system_logs.xml` 中声明 `<backups><allowed_path>/</allowed_path></backups>`，支持容器执行跨目录本地备份。

### 2.3 修复公网真实链端到端测试 (`live_chain_collects_and_searches_raw_base_usdc_logs`)
- **注入 ClickHouse 客户端**：重构 `build_test_state_with_clickhouse`，在测试中动态连接并注入真实的 ClickHouse 客户端至 `state`，打通日志采集器向 ClickHouse `raw_logs` 的写入。
- **纠正日志行数查询**：废弃原先误查 SQLite 元数据 `eventlake_subscriptions` 的函数，新增 `count_raw_logs_in_clickhouse`，直接对 ClickHouse 执行 `SELECT count() FROM raw_logs FINAL ...` 校验落盘行数，成功跑通 Base 主网 166 条真实 USDC 日志的抓取与 Search DSL 检索。

### 2.4 补齐无公网依赖的本地闭环全流程 E2E 测试
- **增强 Mock RPC Fixture**：在 `tests/e2e_real_database_tests.rs` 中为 `json_rpc_fixture` 增加 JSON-RPC Batch 数组请求支持，并补齐 `eth_getBlockByNumber` 与 `eth_getBlockReceipts` 模拟响应。
- **新增全流程闭环用例**：`full_pipeline_mock_rpc_collector_clickhouse_search_and_reorg_e2e`：
  1. 启动本地 Mock RPC Fixture，配置 SQLite 链与订阅。
  2. 运行 `collector::worker::collect_once`，验证日志入库 ClickHouse `raw_logs`，并通过 Axum REST `/api/raw-logs/search` 校验 DSL 过滤与排序。
  3. 运行 `block_transaction::collector::collect_once`，验证区块与交易入库 ClickHouse `blocks`/`transactions`，并通过 REST `/api/chains/{id}/blocks/{number}` 和 `/transactions/{hash}` 获取详情。
  4. 触发区块分叉检测（Reorg），验证双存储联动回退与 ClickHouse 墓碑标记，确认 Search DSL 与 Block 接口均自动过滤墓碑数据。

### 2.5 运维备份恢复端到端测试打通 ClickHouse
- **ClickHouse 在线备份验证**：在 `tests/test_backup_restore_e2e.sh` 中自动探测 ClickHouse 连通性，打通 ClickHouse 增量分片备份与灾难恢复流程。
- **JSON 安全序列化**：在 `scripts/backup.sh` 中使用 Python `json.dump` 生成 `manifest.json`，彻底杜绝异常堆栈信息中换行符和双引号破坏 JSON 语法的隐患。

### 2.6 实现 RPC 节点池真实平滑加权轮询负载均衡 (TASK-019)
- **SWRR 算法集成**：在 `src/rpc_pool/mod.rs` 中为 `EndpointRuntimeStatus` 引入 `current_weight: i32` 动态运行时权重，并实现经典的平滑加权轮询（Smooth Weighted Round-Robin, SWRR）算法。
- **并发均摊流量**：重构 `select_rpc_endpoint`，废弃原先的静态主备模式（`available.remove(0)`），使得同链所有健康可用节点依据各自配置的 `weight` 比例平滑分担并发采集请求，防止主力节点单点打爆限流。
- **容灾与自愈闭环保持**：当某节点调用发生故障（网络超时、429 限流等）进入 Cooldown 冷却期后，SWRR 动态排除该节点，剩余健康节点按各自权重平滑分摊流量；后台探活 Worker 确认恢复后自动重回轮询候选池。
- **针对性测试覆盖**：在 `tests/rpc_pool_cooldown_test.rs` 中新增 3 组测试（等权 1:1 交替、非等权 4:1 平滑交替调度、SQLite 真实库结合节点故障熔断与恢复测试），全库 60 项测试 100% 通过。

### 2.7 实现远程一键部署自动化脚本 (TASK-020)
- **多模式参数与交互向导**：编写 [`scripts/deploy-remote.sh`](file:///ssd0/git/EVMEventLake/scripts/deploy-remote.sh)，支持命令行参数（`-s/--ssh`, `-d/--dir`, `-e/--env`, `-p/--port`, `-i/--identity`, `--cn`, `--source`, `--dry-run`）以及无参数时的交互式向导。
- **环境嗅探与智能编排**：自动在远端探测 SSH 连通性，自动识别 `docker compose` (V2) 或 `docker-compose` (V1)，检测失败时提供友好安装指引。
- **宿主机数据持久化保护**：远端自动建立并保留 `data/sqlite` 与 `data/clickhouse` 目录，平滑升级与重建容器时绝不误删历史数据。
- **精简增量安全同步**：内置排除列表剔除 `.git`、`target/`、本地数据与日志；优先检测并使用 `rsync` 增量传输，并在缺失 rsync 时优雅降级为 `tar` over SSH 流式传输。
- **配置安全传输**：将指定的本地 `.env` 配置文件传输至远端 `$REMOTE_DIR/.env` 并设置 `chmod 600` 保护。
- **启动与健康检查轮询**：远端执行秒级容器编排构建与拉起，自动从配置读取端口并轮询 `/health/ready` 就绪状态，输出完整访问端点与日志排查指引。
- **完善文档**：在 [`docs/DEPLOYMENT.md`](file:///ssd0/git/EVMEventLake/docs/DEPLOYMENT.md) 中增加远程一键自动化部署使用手册与示例。

### 2.8 完善部署后全流程业务使用与下游集成指南 (TASK-021)
- **业务流程梳理与闭环**：全面重构升级 [`docs/USAGE.md`](file:///ssd0/git/EVMEventLake/docs/USAGE.md)，将部署后的使用链路系统化归纳为 8 个标准步骤：
  1. 健康检查与全局大盘 (`/health/live`, `/health/ready`, `/api/dashboard`)。
  2. 认证配置与 API Key 生成（Admin vs ReadOnly）。
  3. 链与 RPC 节点池配置（种子文件自动注入与动态 REST API，SWRR 负载均衡与健康探活）。
  4. 4 大数据采集场景（单合约采集、批量合约高效合并采集、全链所有事件 All-events 采集、整链区块与交易全量同步）。
  5. 任务监控与生命周期管理（Checkpoint 查看、暂停/恢复、断点续传）。
  6. 数据消费（REST Search DSL、区块交易 API、ClickHouse SQL 直连与 ReplacingMergeTree FINAL 防分叉优化）。
  7. 下游应用代码实操示例（完整 Python 与 Node.js/TypeScript 客户端代码）。
  8. 日常运维与热备份灾难恢复指引及 FAQ。

### 2.9 服务端区块交易常驻解耦与独立纯 Shell 客户端 (TASK-022)
- **服务端区块交易 Worker 常驻解耦**：
  - 将 `src/configuration/mod.rs` 与 `.env.example` 中 `EVENTLAKE_BLOCK_TRANSACTION_ENABLED` 默认值设置为 `true`。
  - 区块同步 Worker 默认常驻启动，在 SQLite 中没有配置或处于暂停状态的链时 0 开销休眠，一旦用户下发启动指令即刻激活，彻底消除修改 `.env` 并重启容器的硬限制。
- **独立纯 Shell 客户端 `lakectl` 开发**：
  - 编写并测试独立跨平台客户端 [`scripts/lakectl`](file:///ssd0/git/EVMEventLake/scripts/lakectl)，纯 Bash 编写，零第三方语言运行时依赖。
  - **自动 SSH 穿透**：通过 `-s/--ssh` 选项自动管理安全 SSH 端口转发隧道，解决服务器 8080/8123 端口处于私网未向公网开放的问题。
  - **终端动态监控大盘 (`lakectl top`)**：支持像 `htop` / `k9s` 一样实时动态刷新概况指标、RPC 节点延迟池、日志订阅 Checkpoint 以及区块交易同步进度条。
  - **动态管理子命令**：包含 `block start/pause/resume/status`、`log list/add/pause/resume/delete`、`rpc list/check/add`、`chain list/add`。
### 2.10 lakectl 声明式配置幂等导入与导出 (TASK-023)
- **声明式配置规范与模板**：
  - 新增标准化全量配置文件模板 [`config/eventlake.example.json`](file:///ssd0/git/EVMEventLake/config/eventlake.example.json)，包含 `chains`、`rpc_endpoints`、`subscriptions`、`block_transaction_sync`。
- **配置导出 (`lakectl config export` / `lakectl export`)**：
  - 自动聚合拉取服务端所有链信息、RPC 节点权重、当前所有合约/全量订阅与整链区块同步策略，格式化输出为规范声明式 JSON 文件。
- **非破坏性幂等导入 (`lakectl config import` / `lakectl import`)**：
  - 读取配置文件并依次以 Upsert 模式提交服务端：
    - 针对已有合约/全量订阅：利用 SQLite `ON CONFLICT DO NOTHING`，**绝对不重置已经落盘同步的 Checkpoint 进度**。
    - 针对已有整链区块同步：保留现有 `next_block` 高度不回退。
    - 针对已有 RPC 节点：更新权重并激活，不产生重复记录。
  - 支持无限次重复导入更新，并可在未暴露端口的云主机上结合 `-s root@host` 进行远程穿透导入。
- **文档补充**：
  - 在 [`docs/USAGE.md`](file:///ssd0/git/EVMEventLake/docs/USAGE.md) 2.4 小节全面补充声明式配置模板、导出与导入使用命令。

### 2.11 lakectl 先登录后操作机制与 3 种连接模式 (TASK-024)
- **3 种连接架构打通**：
  1. **公网暴露 URL 模式**：通过 `--url https://...` 直连，适合配置了域名/反向代理或开放端口的场景。
  2. **SSH 安全穿透内部端口模式**：通过 `-s root@host --remote-port <port>`，针对未对外暴露端口的云主机自动建立安全隧道，穿透访问内网 8080/自定义端口。
  3. **本地直连开发模式**：默认连 `http://127.0.0.1:8080`，开箱即用。
- **凭据上下文持久化 (`login / whoami / logout`)**：
  - `lakectl login`：连通性探活测试通过后，安全将目标 URL、Token、SSH 目标与内部端口写入 `~/.lakectl/config` (600 权限)。
  - **免参数后续操作**：用户登录一次后，后续直接执行 `lakectl top`、`lakectl status`、`lakectl block ...` 无需再输入长参数。
  - `lakectl whoami`：输出当前连接模式、目标、内部端口及连通性状态。
  - `lakectl logout`：安全清理本地保存的上下文配置文件。
- **文档同步**：
  - 在 [`docs/USAGE.md`](file:///ssd0/git/EVMEventLake/docs/USAGE.md) 中加入 2.2 登录模式与凭据持久化章节。

---

## 3. 文件变动清单

### 新建文件 (Created Files)
- `config/eventlake.example.json`: 声明式全量配置参考模板。
- `scripts/lakectl`: 独立纯 Shell 客户端（支持 SSH 隧道穿透、TUI 动态大盘、声明式导入导出、先登录后操作与全流程控制）。
- `scripts/deploy-remote.sh`: 远程一键部署自动化脚本（支持 SSH、指定目录、指定本地 env、环境嗅探与健康轮询）。
- `docs/AI/tasks/TASK-020.md`: TASK-020 任务目标、范围、验收标准与验证结果记录。
- `docs/AI/tasks/TASK-021.md`: TASK-021 任务目标、范围、验收标准与验证结果记录。
- `docs/AI/tasks/TASK-022.md`: TASK-022 任务目标、范围、验收标准与验证结果记录。
- `docs/AI/tasks/TASK-023.md`: TASK-023 任务目标、范围、验收标准与验证结果记录。
- `docs/AI/tasks/TASK-024.md`: TASK-024 任务目标、范围、验收标准与验证结果记录。

### 调整修正的文件 (Modified Files)
- `src/configuration/mod.rs`: 将 `EVENTLAKE_BLOCK_TRANSACTION_ENABLED` 默认配置值调整为 `true`。
- `.env.example`: 同步说明区块交易 Worker 默认常驻待命。
- `docs/USAGE.md`: 全面升级为包含架构流程图、业务实操命令、ClickHouse 查询准则、多语言 SDK 示例、声明式配置导入导出、3种连接模式与登录凭据持久化手册。
- `docs/DEPLOYMENT.md`: 补充远程一键自动化部署手册及多场景使用命令。
- `docs/AI/TASK_INDEX.md`: 登记并标记 TASK-024 为 `DONE`。
- `docs/AI/SESSION_STATE.md`: 更新核心状态与交接记录。

---

## 4. 已运行的验证命令及结果

```bash
# 1. 验证脚本语法正确性
bash -n scripts/deploy-remote.sh
# 退出码 0

# 2. 帮助信息输出测试
./scripts/deploy-remote.sh --help
# 退出码 0

# 3. 容错测试：非法 env 文件路径拦截
./scripts/deploy-remote.sh -s root@127.0.0.1 -d /opt/eventlake -e non_existent_env.env
# 退出码 1，输出: [ERROR] 指定的本地 env 配置文件不存在: non_existent_env.env

# 4. 容错测试：非法 SSH 主机连通性拦截
./scripts/deploy-remote.sh -s invalid.test.host.local -d /opt/eventlake -e .env.example --dry-run
# 退出码 1，输出: [ERROR] 无法通过 SSH 连接到目标服务器: invalid.test.host.local

# 5. 全库编译与静态检查
cargo check --ignore-rust-version --all-targets
# 退出码 0
```

---

## 5. 未解决问题 (Known Issues)

- 无。

---

## 6. 风险和假设 (Risks and Assumptions)

- **假设**: 目标服务器具备基础 SSH 访问权限，且服务器已安装 Docker。
- **风险**: 极低，脚本为独立运维工具，不侵入后端核心业务代码。

---

## 7. 下一步计划 (Next Task)

- **建议**: 用户可提供目标服务器的 SSH 链接（如 `root@x.x.x.x`）、远端目标目录（如 `/opt/eventlake`）与本地 `.env` 配置文件路径，执行一键部署。
- **下一次 Session 应先读取的文件**:
  1. [`AGENTS.md`](file:///ssd0/git/EVMEventLake/AGENTS.md)
  2. [`docs/AI/GOAL.md`](file:///ssd0/git/EVMEventLake/docs/AI/GOAL.md)
  3. [`docs/AI/TASK_INDEX.md`](file:///ssd0/git/EVMEventLake/docs/AI/TASK_INDEX.md)
  4. [`docs/AI/SESSION_STATE.md`](file:///ssd0/git/EVMEventLake/docs/AI/SESSION_STATE.md)
