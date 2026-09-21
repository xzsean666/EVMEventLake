# 会话状态记录 (SESSION STATE)

本文档记录当前开发会话的状态，是跨 Session 恢复工作的直接依据。

---

## 1. 核心状态概要

- **当前 Goal**: 建设与完善生产运维与业务落地全流程支持体系
- **当前 Task**: 
  - **TASK-030**: 扩展高级链上分析 API (区块用户 Gas 排行、Gas Oracle、网络统计、巨鲸转账、热门合约与失败交易) (`DONE`)
  - **TASK-029**: 扩展实用区块与交易分析 API (时间查块、时间区间、交易确认数、地址画像与合约部署) (`DONE`)
  - **TASK-028**: 基于可用 RPC 动态并发与节点能力自适应切片流水线 (`DONE`)
  - **TASK-027**: 区块与交易多节点并发分片抓取流水线与切片故障自愈顶替机制 (`DONE`)
  - **TASK-026**: RPC 节点能力感知、Archive/普通节点分流调度与合约 Logs 动态切片策略 (`DONE`)
  - **TASK-025**: 预置 Soneium 主网 Archive RPC 节点与加权配置进入默认与示例配置 (`DONE`)
  - **TASK-024**: lakectl 支持先登录后操作 (login/logout/whoami) 与自定义远程端口 (`DONE`)
  - **TASK-023**: lakectl 支持声明式配置幂等导入与导出 (import/export config) (`DONE`)
  - **TASK-022**: 服务端区块交易常驻解耦与独立纯 Shell 客户端 (lakectl) (`DONE`)
  - **TASK-021**: 完善部署后全流程业务使用与下游集成指南 (USAGE.md) (`DONE`)
  - **TASK-020**: 远程一键部署自动化脚本 (deploy-remote.sh) 与部署文档支持 (`DONE`)
- **当前状态**: `DONE` (已完成 6 个高级链上分析与监控 API 的实现、OpenAPI 声明与 ClickHouse 集成测试验证，全库 65 个测试 100% 通过)

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

### 2.12 预置 Soneium 主网 Archive RPC 节点与加权配置 (TASK-025)
- **实测支持 Archive 历史状态查询**：
  - `https://rpc.soneium.org` (官方公共节点，归档完备，权重 100)
  - `https://1868.rpc.thirdweb.com` (Thirdweb 公共端点，归档完备，单次日志跨度 1000 块，权重 90)
  - `https://nodes.sequence.app/soneium` (Sequence 公共端点，归档完备，权重 85)
  - `https://soneium.drpc.org` (dRPC 公共端点，归档完备，大范围日志受限，权重 70)
- **预置入系统默认配置**：
  - 在 `config/rpc_endpoints.json` 和 `config/rpc_endpoints.json.example` 中补充上述 4 个端点，`total_endpoints` 递增至 88，支持系统启动时无缝种子化 Soneium 节点池。
  - 在 `config/eventlake.example.json` 中增加 Soneium (Chain ID 1868) 的链定义与 RPC 节点声明。

### 2.13 RPC 节点能力感知、Archive/普通节点分流调度与合约 Logs 动态切片策略 (TASK-026)
- **压平基础 Schema (无额外迁移)**：
  - 直接在 `migrations/202609180001_initial_schema.sql` 中的 `eventlake_rpc_endpoints` 增加 `is_archive BOOLEAN NOT NULL DEFAULT 1`、`max_block_range INTEGER`、`max_batch_size INTEGER`，不保留额外递增迁移文件。
- **RPC 节点池调度算法增强 (`src/rpc_pool/mod.rs`)**：
  - 升级 `RpcEndpointRecord`、`CreateRpcEndpointRequest` 与 `RpcEndpointSeed`，支持节点能力标注。
  - 引入 `EndpointRequirements { needs_archive, preferred_block_range }` 及 `select_rpc_endpoint_with_requirements`：
    - **历史追赶**（`needs_archive = true`）：仅向 `is_archive = true` 的节点分发请求，杜绝向修剪节点请求历史引发的 `state pruned` 错误；
    - **实时同步**（`needs_archive = false`）：普通节点（Pruned，如 NodeFlare）与 Archive 节点共同依据权重平滑负载均衡，保护高价值 Archive 节点算力；
    - **推荐跨度优先**：优先调度支持大跨度的节点。
- **日志采集器动态安全切片 (`src/collector/worker.rs`)**：
  - 在 `collect_subscription` 与 `collect_subscription_batch` 中引入动态切片：`effective_window = match endpoint.max_block_range { Some(limit) if limit > 0 => block_window.min(limit), _ => block_window }`。
  - 允许冷门合约 `max_block_window` 配置至百万级（1,000,000 块）高速追赶；当调度至 1000 块限制节点时，请求自动切片，绝不触发超限报错或误折半降速；调度至大跨度节点时即可大步跳跃。
- **区块交易同步批处理自适应 (`src/block_transaction/collector.rs`)**：
  - 依据 `endpoint.max_batch_size` 动态截断单批拉取大小，杜绝大批超时。
- **单元与集成测试覆盖**：
  - 在 `tests/rpc_pool_cooldown_test.rs` 中新增 `test_select_rpc_endpoint_with_archive_and_range_requirements`，验证归档隔离与大跨度优先策略，全量测试全部通过。

### 2.14 区块与交易多节点并发分片抓取流水线与切片故障自愈顶替机制 (TASK-027)
- **历史追赶多节点分片流水线（Multi-Node Chunk Pipeline）**：
  - 实现 `partition_block_range_into_slices` 函数，在历史追赶（`syncing`）且剩余落后区块较多时，自动将抓取区间切分为多个连续切片。
  - 使用 `tokio::task::JoinSet` 并发派发各个切片，利用 RPC 节点池的 SWRR 调度将不同切片自然分散到不同健康节点，实现历史区块与交易的几倍吞吐提速。
- **实时跟进单切片防抖（Realtime Tip）**：
  - 当同步状态处于 `caught_up` 或剩余区块 $\le$ 单批大小时，强制单切片单节点顺序执行，保留严密的 Reorg 探针与父哈希检测，避免在链顶产生不必要的并发竞争与开销。
- **单切片故障自愈顶替机制（Failover Takeover）**：
  - 实现 `fetch_slice_with_failover`，每个切片内部最多支持 5 次动态故障转移重试。
  - 当某个 RPC 节点在抓取区块或批量收据时遭遇网络抖动、超时、429 限流或 502/503 报错，自动调用 `mark_rpc_failure` 将故障节点置入阶梯退避冷却，并由 `select_rpc_endpoint_with_requirements` 选出其他可用健康节点即刻顶替继续重试，直到该切片完整成功。
- **全量完备性保障（All-or-Nothing Completeness）**：
  - 只有当前任务分发的所有切片全部抓取成功，且跨切片通过 `validate_block_sequence`（验证全局严格连续且父哈希完全吻合）并成功批量写入 ClickHouse 后，才原子推进 SQLite Checkpoint。
  - 若最终有切片因所有节点均不可用而失败，整批任务失败并退出，绝不推进 Checkpoint，坚决杜绝区块空洞与漏块。

### 2.15 基于可用 RPC 动态并发与节点能力自适应切片流水线 (TASK-028)
- **解除区块交易历史归档强依赖**：
  - 区块头、交易体与收据属于 EVM 规范的全节点必备存储，无需中间状态树。将 `collect_chain` 中的路由要求调整为 `needs_archive: false`，释放普通全节点（如 NodeFlare）并发算力，Soneium 5 个节点全部投入追赶。
- **活跃健康节点列表暴露 (`src/rpc_pool/mod.rs`)**：
  - 提取并暴露 `get_available_rpc_endpoints_with_requirements`，允许上层收集模块动态获取当前链全部健康、非冷却且符合条件的活跃候选节点列表。
- **节点吞吐能力自适应切片（Capacity-Aware Chunk Slicing）**：
  - 重构 `partition_block_range_into_slices`，不再使用静态切片数和统一批大小，而是根据当前活跃节点的各自能力参数（`max_batch_size`）进行自适应连续切分：
    - 官方节点分配 50 块；
    - Thirdweb、Sequence、NodeFlare 分配 20 块；
    - dRPC 分配 10 块；
    - 单 Tick 瞬间拉取 $50 + 20 + 20 + 20 + 10 = 120$ 块，各节点均工作在最优参数下，无闲置无超载。
- **切片精准指派与动态故障转移**：
  - 重构 `fetch_slice_with_failover`，切片初始直接交由为其量身定制的节点执行；一旦发生故障，记录冷却并由其他健康节点接管，内部依据新节点的 `max_batch_size` 自动进行安全子切片拆分。
- **单元测试与回归验证**：
  - 更新 `tests/block_transaction_test.rs` 中的 `test_partition_block_range_into_slices_historical_and_tip`，全库 8 项 block_transaction 测试、7 项 rpc_pool 测试与全项目编译检查全部通过。

### 2.16 Soneium 节点扩容与全节点链上实测验证 (8 节点并发矩阵)
- **发现并实测 3 个全新端点**：
  - `https://soneium-mainnet.rpc.sentio.xyz` (Sentio: Archive, Batch 50, Logs 10k)
  - `https://soneium.gateway.tenderly.co` (Tenderly: Archive, Batch 20, Logs 10k)
  - `https://rpc.swiftnodes.io/rpc/soneium` (SwiftNodes: Pruned 全节点, Batch 50, 支持区块与收据批量获取)
- **链上真实验证全连通**：
  - 对 8 个节点进行全项探测（最新块高、延迟、历史块 #100、收据获取、Archive 状态、Logs 范围），全部节点真实可用，最新 Head 同步在 `28,380,417~28,380,457` 之间。
- **并发能力倍增**：
  - 8 个节点全部投入区块与交易追赶，单 Tick 历史追赶总并发吞吐量达到 **240 块**。
- **配置文件更新**：
  - `config/rpc_endpoints.json` 端点数增至 92。
  - `config/rpc_endpoints.json.example` 同步更新至 92。
  - `config/eventlake.example.json` 补充了全部 8 个端点定义。

### 2.17 生成 Soneium 专属区块与交易同步配置 (soneium_blocks_transactions.json)
- **配置定制**：
  - 新建 `config/soneium_blocks_transactions.json`，仅包含 Soneium (Chain ID 1868)。
  - 接入已实测验证的全部 8 个加权自适应 RPC 节点矩阵（含官方、Sentio、Thirdweb、Sequence、Tenderly、NodeFlare、SwiftNodes、dRPC）。
  - `subscriptions` 设为空列表 `[]`（不采集任何合约 Logs）。
  - `block_transaction_sync` 启用整链区块与交易同步（`enabled: true`, `realtime_enabled: true`），`start_block` 设置为链上当前实时高度 `28381470`。
  - 完美支持 `lakectl config import` 幂等下发。

### 2.18 远程部署自动化与 Soneium 生产实测全流程验证
- **远程部署自动化实测与缺陷自愈 (`scripts/deploy-remote.sh`)**：
  - **容器非 root 权限自愈**：EventLake 容器以安全非 root 用户 `eventlake:eventlake` (UID 10001) 运行。在首次部署挂载宿主机目录时，`deploy-remote.sh` 自动为 `$REMOTE_DIR/data/sqlite` 赋予写权限 (`chmod -R 777`)，彻底杜绝 SQLite 报 `(code: 14) unable to open database file` 的闪退隐患。
  - **配置文件加载对齐**：将 `docker-compose.yml` 与 `docker-compose.source.yml` 中的 `env_file` 默认值由 `.env.example` 调整为 `.env`；并在 `deploy-remote.sh` 同步规则中保留 `.env.example` 且在启动时导出 `EVENTLAKE_ENV_FILE=.env`，保证生产配置精准生效。
- **lakectl 客户端全方位增强与健壮性提升 (`scripts/lakectl`)**：
  - **位置无关全局参数解析**：优化 `main` 函数，支持全局标志（`-u/--url`, `-s/--ssh`, `-i`, `-p`, `-k` 等）任意放置在子命令之前或之后（例如 `lakectl status --url http://...` 与 `lakectl import config.json -s ...` 均可完美解析）。
  - **声明式配置导出能力补齐**：在 `cmd_config_export` 中增加 `is_archive`、`max_block_range`、`max_batch_size` 字段映射，确保导出的声明式配置包含全部节点自适应能力参数。
- **目标服务器实装部署与链上实测 (192.168.31.33)**：
  - **多项目隔离部署**：成功将项目完整部署于指定多项目根目录下的独立工程子目录 `/home/apps/ems/EVMEventLake`，Docker 29.6.1 + Compose v5.3.1 容器编排秒级拉起，`/health/ready` 就绪通过。
  - **登录态持久化测试**：执行 `./scripts/lakectl login --url http://192.168.31.33:8080`，成功将凭据持久化至 `~/.lakectl/config`，后续 `lakectl whoami`、`lakectl status` 等命令均免输参直连。
  - **精准配置声明式导入**：导入 `config/soneium_blocks_transactions.json`，系统精准仅注册 Soneium (Chain 1868) 与 8 个加权自适应 RPC 节点，确认 `subscriptions: []`（日志采集订阅为 0，严格不采集任何额外合约日志），并激活整链区块与交易同步。
  - **生产运行状态全面观测**：
    - **极速并发吞吐**：在自适应切片流水线驱动下，约 90 秒内高速同步 **1,280+ 个区块** 与 **12,010+ 笔交易**，并成功落盘 ClickHouse `blocks` 与 `transactions` 表。
    - **故障隔离自愈验证**：公共节点 `https://rpc.nodeflare.app/soneium/public` 返回 403 Forbidden，系统自动探活检测并标记 `unhealthy` 实施阶梯冷却；其余 7 个节点平滑分摊算力，切片故障自愈顶替机制无感生效，业务无漏块、无报错中断。
    - **运维控制实测**：`lakectl block pause 1868` 与 `lakectl block resume 1868` 断点续传平滑无缝；`lakectl rpc check` 正确测试所有端点并输出毫秒级延迟；`lakectl export` 完整逆向导出集群全量状态。

### 2.19 ClickHouse 深度日志收敛与磁盘防爆治理 (Warning 级别调优、20M 轮转、1 天 TTL 与历史释放)
- **文件日志极致收敛 (`<logger>`)**：在 `clickhouse/config.d/system_logs.xml` 中显式配置 `<logger>`，将文件日志级别提升至 `warning`（彻底去除 `trace`/`debug`/`information` 等常规信息，仅记录警告与错误），并将单文件上限压缩至 `20M`，仅保留 `1` 份轮转。
- **系统表保留期统一缩短为 1 天**：
  - 将 `query_log`、`part_log`、`text_log`、`metric_log`、`asynchronous_metric_log`、`asynchronous_insert_log` 的 TTL 统一配置为 `event_date + INTERVAL 1 DAY DELETE`。
  - 在 `<text_log>` 中同样配置 `<level>warning</level>`，杜绝内部常规信息注入系统审计表。
- **彻底移除冗余高频审计日志**：
  - 增加 `<processors_profile_log remove="1"/>` 与 `<opentelemetry_span_log remove="1"/>`，连同已有的 `<trace_log remove="1"/>`，根除高频写入下的无用内部采样开销。
- **远程服务器实机落地与历史空间释放**：
  - 同步配置文件至 `192.168.31.33`，截断历史 391MB 的 `clickhouse-server.log`，清空历史积压系统表并重启生效。
  - **优化成效**：
    - 项目整体磁盘占用由 **1016 MB** 骤降至 **504 MB**（降幅达 **50.4%**）；
    - `logs/clickhouse` 目录占用由 **391 MB** 降至 **216 KB**（其中活跃日志仅 **31 KB**，降幅 **>99.9%**）；
    - `system.text_log` 由 188 万行 (67 MB) 压缩表骤降至数十行警告信息；
    - ClickHouse 容器与服务秒级恢复 `healthy`，API 连通与区块采集无缝持续运行。
- **规范与指南文档同步更新**：
  - 全面更新 [`docs/AI_CLICKHOUSE_DIRECTIVE.md`](file:///ssd0/git/EVMEventLake/docs/AI_CLICKHOUSE_DIRECTIVE.md) 与 [`docs/CLICKHOUSE_OPTIMIZATION_GUIDE.md`](file:///ssd0/git/EVMEventLake/docs/CLICKHOUSE_OPTIMIZATION_GUIDE.md)。

### 2.20 扩展实用区块与交易分析 API (TASK-029)
- **落地 5 个高价值实用 REST API**：
  1. **按时间戳查最近区块 (`GET /api/chains/{chain_id}/block-by-time`)**：
     - 支持 `timestamp` 与 `closest` (`before` / `after`)，秒级在 ClickHouse 中根据时间戳定位就近规范区块，自动过滤 Reorg 产生的分叉区块。
  2. **时间区间转区块高度 (`GET /api/chains/{chain_id}/blocks-time-range`)**：
     - 支持输入秒级起止时间戳 `start_time` 与 `end_time`，利用 `minOrNull`/`maxOrNull` 聚合返回对应区间的最小高度、最大高度以及总出块数 `block_count`。
  3. **轻量交易确认数与状态 (`GET /api/chains/{chain_id}/transactions/{tx_hash}/status`)**：
     - 单次查询返回交易执行状态 `status` (1/0)、Gas 消耗、所在区块高度、当前链顶高度 `current_head` 与精确确认数 `confirmations`。
  4. **地址轻量画像 (`GET /api/chains/{chain_id}/addresses/{address}/profile`)**：
     - 一条 SQL 高效聚合地址的首次活跃高度 `first_block`、最近活跃高度 `last_block`、作为发送方交易数 `sent_tx_count`、作为接收方交易数 `received_tx_count` 以及最后使用的 `last_nonce`。
  5. **合约部署（打新）交易发现 (`GET /api/chains/{chain_id}/deployments`)**：
     - 基于 EVM 合约部署特征（`to_address IS NULL`），高效检索全网或指定创建者 `creator` 的合约创建交易列表，支持完整 keyset 游标分页。
- **SQLite RPC 节点幂等种子化修复 (`src/rpc_pool/mod.rs`)**：
  - 在 `ON CONFLICT (chain_id, url) DO UPDATE SET` 增加 `WHERE` 变更对比条件，消除重复执行种子化时非必要的行更新，确保幂等重复种子化返回 0 影响行数。
- **全量测试与实时 ClickHouse 集成测试验证**：
  - 在 `tests/block_transaction_test.rs` 增加模型序列化与参数校验单元测试；
  - 在 `tests/clickhouse_integration_tests.rs` 增加 5 个新端点基于真实 ClickHouse 容器的端到端集成测试；
  - 全库 64 个测试 100% 通过（含单元测试、ClickHouse 存储测试、E2E 测试、真实链测试与 RPC 节点池调度测试）。

### 2.21 扩展高级链上分析与监控 API (TASK-030)
- **落地 6 个高级链上分析与监控 REST API**：
  1. **区块内用户 Gas 消耗排行榜 (`GET /api/chains/{chain_id}/blocks/{block_ref}/gas-consumers`)**：
     - 单次查询聚合区块内所有交易，按 `from_address` 汇总各用户总消耗的 Gas（`total_gas_used`）及交易笔数（`tx_count`），降序排列，直观洞察区块 Gas 巨鲸。
  2. **实时 Gas 预测器 (`GET /api/chains/{chain_id}/gas-oracle`)**：
     - 基于最新区块 `base_fee_per_gas` 及最近 20 个区块交易的优先费分位数（20th 慢速、50th 正常、80th 快速），自动估算建议的 `max_priority_fee_per_gas` 与 `max_fee_per_gas`。
  3. **实时网络健康统计 (`GET /api/chains/{chain_id}/network-stats`)**：
     - 单条高效聚合查询返回当前链最新高度、时间戳、最近 1 小时 TPS（`tps_last_1h`）、最近 100 块平均 Gas 饱和度（`avg_gas_utilization_percent`）以及平均出块耗时（`avg_block_time_seconds`）。
  4. **大额巨鲸转账监控 (`GET /api/chains/{chain_id}/whale-transfers`)**：
     - 基于 ClickHouse `toUInt256OrZero(value)` 大数运算筛选转账金额 $\ge$ `min_value`（默认 1 ETH）的交易流，支持完全防篡改的不透明游标 keyset 分页。
  5. **热门合约排行榜 (`GET /api/chains/{chain_id}/top-contracts`)**：
     - 在指定滑窗区块数（`window_blocks`，默认 1000）内，按被调用合约地址聚合统计调用量（`tx_count`）、独立交互用户数（`user_count`）及总消耗 Gas（`total_gas_used`）。
  6. **失败交易排查流 (`GET /api/chains/{chain_id}/failed-transactions`)**：
     - 筛选 `status = 0` 的执行失败交易流，支持游标 keyset 分页，便于监控异常合约或网络拥堵引发的 revert 峰值。
- **全量测试与实时 ClickHouse 集成测试验证**：
  - 在 `tests/block_transaction_test.rs` 增加高级模型单元测试；
  - 在 `tests/clickhouse_integration_tests.rs` 增加 6 个新端点基于真实 ClickHouse 容器的端到端集成测试，严格校验排行顺序、百分比与游标分页；
  - 全库 65 个测试 100% 通过；
  - 编译更新本地 Release 优化版本至 `deploy/prebuilt/eventlake`。

---

## 3. 文件变动清单

### 新建文件 (Created Files)
- `docs/AI/tasks/TASK-030.md`: TASK-030 任务目标、范围、验收标准与验证结果记录。
- `docs/AI/tasks/TASK-029.md`: TASK-029 任务目标、范围、验收标准与验证结果记录。
- `config/soneium_blocks_transactions.json`: Soneium 专属声明式导入配置（仅同步区块与交易）。
- `docs/AI/tasks/TASK-028.md`: TASK-028 任务目标、范围、验收标准与验证结果记录。
- `docs/AI/tasks/TASK-027.md`: TASK-027 任务目标、范围、验收标准与验证结果记录。
- `docs/AI/tasks/TASK-026.md`: TASK-026 任务目标、范围、验收标准与验证结果记录。
- `docs/AI/tasks/TASK-025.md`: TASK-025 任务目标、范围、验收标准与验证结果记录。

### 调整修正的文件 (Modified Files)
- `src/clickhouse/block_transaction.rs`: 新增 `get_block_gas_consumers`, `get_gas_oracle`, `get_network_stats`, `get_whale_transfers`, `get_top_contracts`, `get_failed_transactions` 6 个 ClickHouse 聚合与点查函数，完善 Nullable 与数值类型映射。
- `src/clickhouse/mod.rs`: 重新导出新查询函数与 Row 结构体。
- `src/block_transaction/api.rs`: 注册 6 个新 REST 端点、定义请求/响应结构体与 OpenAPI 规范。
- `tests/block_transaction_test.rs`: 增加新 API 请求/响应模型与参数校验单元测试。
- `tests/clickhouse_integration_tests.rs`: 补充 6 个新 API 基于真实 ClickHouse 容器的端到端集成测试。
- `deploy/prebuilt/eventlake`: 重新编译更新最新的 Release 预构建二进制。
- `docs/AI/TASK_INDEX.md`: 登记并标记 TASK-030 为 `DONE`。
- `docs/AI/SESSION_STATE.md`: 更新核心状态与交接记录。

---

## 4. 已运行的验证命令及结果

```bash
# 1. 编译与类型检查
cargo check --ignore-rust-version --all-targets
# 退出码 0

# 2. block_transaction 单元测试
cargo test --ignore-rust-version --test block_transaction_test
# 结果: 10 passed, 0 failed

# 3. 真实 ClickHouse 容器集成测试（含全部 11 个新端点与 Reorg 墓碑）
EVENTLAKE_RUN_CLICKHOUSE_INTEGRATION=true cargo test --ignore-rust-version --test clickhouse_integration_tests
# 结果: 2 passed, 0 failed

# 4. 全库回归测试
cargo test --ignore-rust-version
# 结果: 65 passed, 0 failed (100% 通过)

# 5. Release 本地编译
cargo build --release --ignore-rust-version && cp target/release/eventlake deploy/prebuilt/eventlake
# 退出码 0
```

---

## 5. 未解决问题 (Known Issues)

- `https://rpc.nodeflare.app/soneium/public` 返回 403 Forbidden（属于公网限流或需要 API Key），系统节点池已自动将其熔断冷却隔离，其余 7 个节点平稳运行。

---

## 6. 风险和假设 (Risks and Assumptions)

- **假设**: 目标服务器网络可访问公网 EVM RPC 端点。
- **风险**: 极低，Soneium 拥有 7 个健康活跃节点，SWRR 加权轮询与故障自愈机制保证数据完整连续。

---

## 7. 下一步计划 (Next Task)

- **建议**: 新增的 6 个高级 API 已全部通过 ClickHouse 真实容器集成测试与全库测试，本地 Release 预构建二进制已更新。可在远程服务器上执行一键部署更新。
- **下一次 Session 应先读取的文件**:
  1. [`AGENTS.md`](file:///ssd0/git/EVMEventLake/AGENTS.md)
  2. [`docs/AI/GOAL.md`](file:///ssd0/git/EVMEventLake/docs/AI/GOAL.md)
  3. [`docs/AI/TASK_INDEX.md`](file:///ssd0/git/EVMEventLake/docs/AI/TASK_INDEX.md)
  4. [`docs/AI/SESSION_STATE.md`](file:///ssd0/git/EVMEventLake/docs/AI/SESSION_STATE.md)

