# TASK-025: 预置 Soneium 主网 Archive RPC 节点与加权配置进入默认与示例配置

## Objective
将实测支持 Archive 历史状态查询的 Soneium 链（Chain ID: 1868）公共 RPC 节点预置入系统默认种子文件 `config/rpc_endpoints.json`、示例种子文件 `config/rpc_endpoints.json.example` 以及声明式全量配置模板 `config/eventlake.example.json`，并依据各节点的归档能力、日志跨度限制与稳定性配置合理权重。

## Scope
- 包含：
  - 更新 `config/rpc_endpoints.json`，增加 4 个 Soneium 端点（官方公共节点、Thirdweb、Sequence、dRPC）及其加权，更新总节点数和时间戳。
  - 更新 `config/rpc_endpoints.json.example`，同步 4 个 Soneium 端点配置。
  - 更新 `config/eventlake.example.json`，补充 Soneium 链定义与 RPC 节点声明。
- 不包含：
  - 修改 Rust 核心业务与数据库表结构。

## Allowed Files
- `config/rpc_endpoints.json`
- `config/rpc_endpoints.json.example`
- `config/eventlake.example.json`
- `docs/AI/tasks/TASK-025.md`
- `docs/AI/TASK_INDEX.md`
- `docs/AI/SESSION_STATE.md`

## Dependencies
- 前置任务：TASK-019（RPC 节点平滑加权轮询 SWRR 算法已就绪）
- 外部依赖：None

## Inputs and Outputs
- **Inputs**:
  - 实测验证的 Soneium RPC 端点 URL、归档能力与响应特征：
    - `https://rpc.soneium.org` (weight: 100)
    - `https://1868.rpc.thirdweb.com` (weight: 90)
    - `https://nodes.sequence.app/soneium` (weight: 85)
    - `https://soneium.drpc.org` (weight: 70)
- **Outputs**:
  - 更新后的 `config/rpc_endpoints.json`、`config/rpc_endpoints.json.example`、`config/eventlake.example.json`。

## Acceptance Criteria
- [x] `config/rpc_endpoints.json` 包含 Soneium 4 个端点，权重设置合理，JSON 语法合法，`total_endpoints` 正确更新为 88。
- [x] `config/rpc_endpoints.json.example` 同步更新。
- [x] `config/eventlake.example.json` 包含 Soneium (Chain ID 1868) 的链定义与 RPC 示例。
- [x] 系统配置校验与现有单元测试通过。

## Verification Commands
```bash
python3 -m json.tool config/rpc_endpoints.json > /dev/null
python3 -m json.tool config/rpc_endpoints.json.example > /dev/null
python3 -m json.tool config/eventlake.example.json > /dev/null
cargo test --test validation_tests
```

## Risks and Assumptions
- 风险：无。配置文件向后兼容且符合现有数据结构。
- 假设：Soneium 链 ID 为 1868，原生代币为 ETH。

## Status
DONE
