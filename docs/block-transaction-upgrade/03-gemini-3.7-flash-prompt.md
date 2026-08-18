# Gemini 3.7 Flash 执行 Prompt

将下面整段内容交给 Gemini 3.7 Flash。它是一个“按任务执行”的工程 prompt，要求模型
在同一个仓库中逐项完成代码、测试和文档；本文件本身不是代码实现。

```text
你是 EVMEventLake 仓库的 Rust 后端工程师。请在当前工作区实现 block / transaction
数据采集与查询 API 升级。先阅读并遵守：

1. docs/block-transaction-upgrade/01-upgrade-design.md
2. docs/block-transaction-upgrade/02-task-breakdown.md
3. docs/evm-block-transaction-data-collection-architecture.md
4. docs/ARCHITECTURE.md
5. docs/CLICKHOUSE_UPGRADE.md

目标：通过 eth_getBlockByNumber(blockNumber, true) 批量采集 canonical EVM blocks 和
transactions，保存到 ClickHouse，并提供基于已采集字段的 API。PostgreSQL 只保存同步
状态、RPC、认证和 reorg 协调。现有 raw-log 功能必须保持兼容。

重要事实：仓库中的 src/collector 已经是现成的 log 收集模块。不要新增、替换或重写 log
collector；不要改动 eth_getLogs、raw-log checkpoint、raw-log reorg 或 raw-log API。新
功能必须使用独立的 src/block_transaction/ 模块和独立 worker；只有抽取有测试保护的
通用 helper 时才允许触碰现有 collector 文件。

硬性范围：

- 只实现 blocks、transactions、checkpoint、reorg 和文档中列出的 API。
- 不实现 ABI 解码、完整 input/calldata、transaction receipt、status、gas_used、logs、
  traces、internal transactions 或 token transfer。
- 不把 block/transaction 写入 eventlake_raw_logs，不恢复 decoder，不在 ClickHouse 不可用
  时静默回退 PostgreSQL。
- 复用现有 Axum、SQLx、reqwest、ApplicationError、ApplicationState、RPC endpoint 选择、
  auth、pagination、tracing 和 ClickHouse client 模式。
- 使用 apply_patch 编辑文件；保留工作区中不是你创建的改动；不要 git reset/checkout。
- 所有 DDL/migration 幂等，所有写入可重试，所有查询只读 canonical latest version。
- uint256、value、nonce、gas、fee 等 API 字段必须返回十进制字符串；可选 RPC 字段缺失
  返回 null；地址/hash 统一 0x 小写和严格长度。

必须实现的 MVP API：

- GET /api/chains/{chain_id}/blocks/{block_ref}
- GET /api/chains/{chain_id}/blocks/{block_ref}/transactions
- GET /api/chains/{chain_id}/transactions/{tx_hash}
- GET /api/chains/{chain_id}/addresses/{address}/transactions

使用当前项目的 ApiResponse(success/data/error/meta)，加认证、OpenAPI、400/404/503 语义。
区块交易列表按 transaction_index 升序，地址交易列表按 block_number、transaction_index、
tx_hash 稳定降序；使用不透明 keyset cursor，禁止 offset 深分页。ClickHouse 查询用 FINAL
或等价 latest-version 查询，并过滤 is_canonical=true。block detail 不嵌套完整交易数组。

按以下任务顺序执行，不能跳过依赖：

T0 现状盘点：只读代码和迁移，列出拟改文件、冲突点、测试命令。不要在不了解现有代码前
创建平行 RPC client 或重复 auth/response 类型。

T1 RPC 类型和 batch：实现严格的 Block/Transaction serde 类型、单块和 JSON-RPC batch，按
request id 匹配响应；处理 miner/author、可选 EIP-1559/Shanghai/Cancun 字段、to=null、
空 input 和 method_id；拒绝非法 hex/长度/溢出；为错序 response、单项 RPC error、部分
成功写单测。不要写数据库。

T2 PostgreSQL state：新增独立的每链 block/transaction sync state migration 和查询模型，
包含 next_block、safe_head、latest_seen_block、status、last_error、timestamps；不要复用
subscription checkpoint，不要改变 raw-log 表语义。明确 start_block 的包含语义和初始化
方式。

T3 ClickHouse：新增 blocks、transactions 幂等 DDL（ReplacingMergeTree(stored_at) 或等价），
canonical/version 字段、合理 ORDER BY、hash/address indexes 或 projection；实现显式列的
批量 writer、FINAL 查询函数和 ClickHouse integration tests。确认区块交易列表、tx hash、
address 查询计划，不要凭感觉声明性能达标。

T4 Block/transaction collector：接入独立后台 worker。只采集 safe_head；batch size、并发、最大响应字节、
reorg window 配置化；复用 select_rpc_endpoint；校验 block number/hash/parent 连续性；
ClickHouse 完整写入成功后才推进 checkpoint；RPC/CH 失败重试相同范围；部分 batch 不能
跳过高度；检测 reorg 时写 tombstone/replacement、暂停到成功；默认开关关闭且不影响旧
log worker。不要把 block/transaction 逻辑添加到 src/collector/worker.rs。

T5 API：新增上述四个 MVP routes、OpenAPI schemas、分页 cursor、参数校验、canonical FINAL
查询、认证和错误映射。ClickHouse 未启用/不可用返回明确 503/502，不回退 PostgreSQL。

T6 运维：增加 admin-only sync 配置、pause/resume 和 read-only sync status；更新配置解析、
日志/指标、.env.example 和 docs/USAGE.md。任何默认从 0 全量同步的隐式行为都禁止。

T7 验证：运行默认 feature 和 clickhouse feature 的 fmt/check/test；真实服务可用时运行
PostgreSQL/ClickHouse integration tests，覆盖重复写、重启、checkpoint failure、reorg、
404/503、cursor 和大整数 JSON；更新架构/OpenAPI/使用文档。无法运行的命令要明确说明原因。

每个任务完成后暂停并输出以下交接，不要把多个任务压成一次无法审查的大改动：

任务：Tn
修改文件：...
实现摘要：...
验证命令及结果：...
已知限制：...
下一任务前置条件：...

实现风格要求：

- 先 rg/read 现有实现，再编辑；小步 apply_patch。
- 优先复用本地 helper 和错误类型；新增抽象只有在确实减少复杂度时使用。
- SQL 字段使用显式列；用户输入只经过白名单字段/operator 进入查询构造。
- 不用字符串拼接伪造 JSON-RPC；不把超大整数转 f64/u64 造成精度丢失。
- 记录结构化日志中的 chain_id、range、endpoint id、batch、attempt、写入行数和错误；不记录
  RPC URL 中的密码或敏感 header。
- 任何发现的工作区已有改动都保留；若与本任务冲突，先报告文件和证据再继续。

最终交付前请给出：变更文件清单、API 示例、数据库/ClickHouse schema 摘要、reorg 与
checkpoint 语义、所有验证结果、未执行验证及原因、容量/查询性能的实测数据或明确的待测项。
```

## Prompt 设计说明

这个 prompt 把 Gemini 的工作切成有明确上下文边界的 T0-T7：T1/T2/T3 是相对独立且容易
测试的基础任务，T4/T5 才接入高风险的状态和 API，T6/T7 负责运维收尾。每一步都要求先
读现有实现、限制改动文件并回报验证结果，降低一次生成大规模代码时遗漏 reorg、精度和
ClickHouse `FINAL` 语义的风险。
