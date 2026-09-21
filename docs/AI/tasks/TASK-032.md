# TASK-032: 关闭主动 RPC 探活、实操报错自动 CD 容灾、客户端注入 User-Agent、API 端口收敛本地 127.0.0.1 并完成远端平滑升级

## 1. 任务背景与目标

- **背景**:
  1. 系统后台探活 Worker 与区块采集共用 `EVENTLAKE_WORKER_TICK_SECONDS=5`，导致部署在 AWS EC2 上的公网 IP 每 5 秒对所有 RPC 并发探测，触发了公共 RPC（Thirdweb 429、Nodeflare 403）的风控限流；且轮询时未判断 `cooldown_until`，导致限流节点被无限重试、日志刷屏。
  2. 用户明确要求：无需主动定时探活，仅在业务实际使用调用报错时触发指数退避冷却（CD），冷却到期后自动参与调度，调用成功自动恢复健康；
  3. 当前部署机器属于内部工作索引节点，不需要外部公网直接访问端口，需收敛 API 绑定至 `127.0.0.1` 本地回环；
  4. 部署远端时排查发现远端数据库历史 Schema 存在 SQLx 迁移校验冲突（`migration 202609180001 was previously applied but has been modified`），需进行平滑自愈修复。
- **目标**:
  1. 解耦并默认关闭主动 RPC 探活 Worker (`EVENTLAKE_RPC_HEALTHCHECK_ENABLED=false`)；
  2. 优化 SWRR 调度，支持非 disabled 且冷却到期的节点按需调度；实操失败自动进 CD，成功自动恢复 healthy；
  3. HTTP 客户端注入标准 User-Agent；
  4. 编排端口映射改为 `127.0.0.1:PORT:PORT`；
  5. 修复远端 SQLite 迁移冲突，重新编译 Release 二进制并一键更新远端；
  6. 全面审查验证远端数据收集进度与日志健康度。

---

## 2. 详细修改内容

1. **配置与后台 Worker 解耦**：
   - `src/configuration/mod.rs`：在 `BackgroundConfiguration` 中增加 `rpc_healthcheck_enabled: bool`，从环境变量 `EVENTLAKE_RPC_HEALTHCHECK_ENABLED` 读取，默认 `false`。
   - `src/background/mod.rs`：仅在 `state.configuration.background.rpc_healthcheck_enabled` 为 true 时才启动探活协程。
2. **SWRR 按需调度与自愈**：
   - `src/rpc_pool/mod.rs`：候选池过滤逻辑调整为 `ep.cooldown_remaining_seconds.is_none() && ep.status != "disabled"`。冷却到期后自然参与 SWRR，实测报错触发退避 CD，调用成功重置为 healthy。
3. **User-Agent 注入**：
   - `src/app/application_state.rs`：`Client::builder()` 显式配置 `EventLake/1.0.0 (+https://github.com/Early-Summer-Studio/soneium-points-indexer)`。
4. **端口本地回环收敛**：
   - `docker-compose.yml` & `docker-compose.source.yml`：将 `ports` 调整为 `"127.0.0.1:${EVENTLAKE_HTTP_PORT:-8080}:${EVENTLAKE_HTTP_PORT:-8080}"`。
5. **远端 SQLite 迁移修复**：
   - 远程执行 SQLite DDL 补齐 `is_archive`、`max_block_range`、`max_batch_size` 字段，并同步更新 `eventlake_sqlx_migrations` 校验和。

---

## 3. 验收验证结果

1. **单元测试与回归测试**: 65 项测试全部通过（含单元、集成与端到端测试）。
2. **Release 编译构建**: 生成 19MB 优化二进制至 `deploy/prebuilt/eventlake`。
3. **远程部署执行**: 容器秒级拉起，`/health/ready` 就绪通过；`ss -tlpn` 确认端口严格仅在 `127.0.0.1:10010` 监听。
4. **日志纯净度**: 高频主动探活日志彻底消除，无异常刷屏。
5. **ClickHouse 收集情况**:
   - `blocks` 快速持续增长至 8,880+；
   - `transactions` 快速持续增长至 96,100+；
   - 收集速率稳定在 ~24 块/秒，业务同步完全正常。
