# AI 提示词模板：生成全量多链公开免费 RPC 候选连接池

本文档是一个即插即用的 **AI 提示词（Prompt Template）**。你可以将以下提示词完整复制并发送给任何大模型（如 Gemini、ChatGPT、Claude 等），让其**针对每条链尽可能多地收集并整理所有已知可靠、可用的公开免费 RPC 节点**（无需 AI 运行 Docker 或任何命令，直接输出纯 JSON）。

拿到 JSON 后，你可以直接使用项目内置的并发测速脚本（`scripts/test-rpc-endpoints.py`）秒级对所有几百个端点进行批量并发测试与清洗，将健康节点一键注入 RPC 池。

---

## 📋 复制以下提示词发送给 AI

```markdown
你现在是一位精通 Web3 基础设施和多链 EVM 架构的资深区块链运维专家。

请为我生成一份用于 EVM 数据索引服务（EVMEventLake）的高可用多链 `rpc_endpoints.json` 种子配置文件。

【重要原则】：
1. **多节点候选池（越多越好）**：不要只提供 1~2 个，请为每条主流链**尽可能多地收集 5 ~ 10+ 个不同的独立公开免费 RPC 节点**（涵盖官方基础节点、PublicNode、1RPC、LlamaNodes、Cloudflare、DRPC 公开端点、Flashbots、MevBlocker、BlockPI 公共节点、Bware Labs/BlastAPI、Tenderly 公共节点等各大主流服务商）。
2. **完全公开免费且免 API Key**：必须是无需注册私有 Key 即可直接发送请求的公开端点，必须支持标准 EVM 查询（`eth_blockNumber`、`eth_getBlockByNumber`、`eth_getLogs`）。
3. **分层权重设计**：
   - 官方与顶级公共节点（如 publicnode、官方 foundation）：权重设为 `100`；
   - 知名大型公共服务商（如 1rpc、llama、drpc、cloudflare）：权重设为 `80 ~ 90`；
   - 社区备用与备选节点：权重设为 `60 ~ 70`。
4. **无需运行环境**：你只需要输出纯 JSON 数据内容，无需启动 Docker 或执行命令。
5. **包含生成时间戳（必填）**：在 JSON 根节点和每个 endpoint 对象中带上当前的 ISO-8601 生成时间戳字段 `"updated_at": "YYYY-MM-DDTHH:MM:SSZ"`（例如 `"2026-08-20T11:25:00Z"`），便于追踪更新鲜度。
6. **格式严格**：直接输出合法的标准 JSON 对象 `{"updated_at": "...", "endpoints": [ ... ]}`，不包含任何 Markdown 说明包裹或尾随逗号，以便直接保存为 `.json` 文件。

---

### 单个节点数据结构（Schema）
```json
{
  "chain_id": 1,
  "url": "https://ethereum-rpc.publicnode.com",
  "weight": 100,
  "chain_name": "Ethereum",
  "native_token_symbol": "ETH",
  "updated_at": "2026-08-20T11:25:00Z"
}
```
字段说明：
- `chain_id`（整数，必填）：EVM Chain ID。
- `url`（字符串，必填）：RPC 的 HTTPS/HTTP 地址。
- `weight`（整数，可选，默认 100）：优先级权重（100 / 90 / 80 / 60 等）。
- `chain_name`（字符串，可选）：链名称（如 "Ethereum", "Base", "Arbitrum One"）。
- `native_token_symbol`（字符串，可选）：原生 Gas 代币（如 "ETH", "BNB", "POL", "AVAX"）。
- `updated_at`（字符串，必填）：生成或验证时间戳（ISO-8601 UTC 格式）。

---

### 需要覆盖的主流区块链清单（请为每个网络尽可能多列出可用的不同 RPC 供应商节点）

#### 1. 主流 Layer 1 主网
- **Ethereum** (Chain ID: 1, Symbol: ETH) —— *请尽可能提供 8~10+ 个知名公共节点*
- **BNB Smart Chain (BSC)** (Chain ID: 56, Symbol: BNB) —— *请提供官方与各类公共备用节点 6~10 个*
- **Polygon PoS** (Chain ID: 137, Symbol: POL) —— *请提供 6~10 个公共节点*
- **Avalanche C-Chain** (Chain ID: 43114, Symbol: AVAX) —— *请提供 5~8 个公共节点*
- **Fantom / Sonic** (Chain ID: 250 / 146, Symbol: FTM / S)
- **Gnosis Chain** (Chain ID: 100, Symbol: xDAI)
- **Cronos** (Chain ID: 25, Symbol: CRO)
- **Celo** (Chain ID: 42220, Symbol: CELO)
- **Kava EVM** (Chain ID: 2222, Symbol: KAVA)
- **Moonbeam** (Chain ID: 1284, Symbol: GLMR)
- **Moonriver** (Chain ID: 1285, Symbol: MOVR)
- **Core DAO** (Chain ID: 1116, Symbol: CORE)
- **Kaia (formerly Klaytn)** (Chain ID: 8217, Symbol: KAIA)

#### 2. 主流 Layer 2 & Rollups (Optimistic & ZK)
- **Arbitrum One** (Chain ID: 42161, Symbol: ETH) —— *请尽可能提供 6~10 个不同 RPC*
- **Arbitrum Nova** (Chain ID: 42170, Symbol: ETH)
- **Base** (Chain ID: 8453, Symbol: ETH) —— *请尽可能提供 6~10 个不同 RPC*
- **OP Mainnet (Optimism)** (Chain ID: 10, Symbol: ETH) —— *请提供 6~10 个不同 RPC*
- **Blast** (Chain ID: 81457, Symbol: ETH)
- **Mantle** (Chain ID: 5000, Symbol: MNT)
- **Linea** (Chain ID: 59144, Symbol: ETH)
- **Scroll** (Chain ID: 534352, Symbol: ETH)
- **zkSync Era** (Chain ID: 324, Symbol: ETH)
- **Polygon zkEVM** (Chain ID: 1101, Symbol: ETH)
- **Taiko** (Chain ID: 167000, Symbol: ETH)
- **Mode** (Chain ID: 34443, Symbol: ETH)
- **World Chain** (Chain ID: 480, Symbol: ETH)
- **Zora** (Chain ID: 7777777, Symbol: ETH)
- **Metis** (Chain ID: 1088, Symbol: METIS)
- **Manta Pacific** (Chain ID: 169, Symbol: ETH)
- **BOB (Build on Bitcoin)** (Chain ID: 60808, Symbol: ETH)

#### 3. 常用测试网 (Testnets)
- **Ethereum Sepolia** (Chain ID: 11155111, Symbol: ETH)
- **Ethereum Holesky** (Chain ID: 17000, Symbol: ETH)
- **Base Sepolia** (Chain ID: 84532, Symbol: ETH)
- **Arbitrum Sepolia** (Chain ID: 421614, Symbol: ETH)
- **OP Sepolia** (Chain ID: 11155420, Symbol: ETH)
- **Polygon Amoy** (Chain ID: 80002, Symbol: POL)
- **BSC Testnet** (Chain ID: 97, Symbol: tBNB)

---

### 输出格式示例

```json
{
  "updated_at": "2026-08-20T11:25:00Z",
  "total_endpoints": 84,
  "endpoints": [
    {
      "chain_id": 1,
      "url": "https://ethereum-rpc.publicnode.com",
      "weight": 100,
      "chain_name": "Ethereum",
      "native_token_symbol": "ETH",
      "updated_at": "2026-08-20T11:25:00Z"
    },
    {
      "chain_id": 1,
      "url": "https://1rpc.io/eth",
      "weight": 90,
      "chain_name": "Ethereum",
      "native_token_symbol": "ETH",
      "updated_at": "2026-08-20T11:25:00Z"
    },
    {
      "chain_id": 1,
      "url": "https://eth.drpc.org",
      "weight": 80,
      "chain_name": "Ethereum",
      "native_token_symbol": "ETH",
      "updated_at": "2026-08-20T11:25:00Z"
    },
    {
      "chain_id": 8453,
      "url": "https://mainnet.base.org",
      "weight": 100,
      "chain_name": "Base",
      "native_token_symbol": "ETH",
      "updated_at": "2026-08-20T11:25:00Z"
    },
    {
      "chain_id": 8453,
      "url": "https://base-rpc.publicnode.com",
      "weight": 90,
      "chain_name": "Base",
      "native_token_symbol": "ETH",
      "updated_at": "2026-08-20T11:25:00Z"
    }
  ]
}
```

请按上述格式输出尽可能完整、丰富的多节点 JSON 列表。
```

---

## ⚡ 生成后的使用与并发测速验证

### 1. 保存 JSON 文件
将 AI 输出的大量候选节点 JSON 保存至 `config/rpc_endpoints.json`：
```bash
mkdir -p config
cat << 'EOF' > config/rpc_endpoints.json
[
  ... AI 生成的大量节点 JSON ...
]
EOF
```

### 2. （无需启动 Docker）使用内置脚本秒级并发测速与清洗
使用项目内置的高并发检测脚本 [`scripts/test-rpc-endpoints.py`](../scripts/test-rpc-endpoints.py)，默认以 **20 并发**（可通过 `--concurrency 50` 进一步提高并发）同时探测所有节点的真实连通性与响应毫秒数：

```bash
# 20 并发秒级测完上百个节点（无需 Docker 或数据库）
python3 scripts/test-rpc-endpoints.py config/rpc_endpoints.json
```

输出示例：
```text
🚀 Concurrently testing 100+ RPC endpoints with concurrency=20 (timeout=5.0s)...

Chain ID   Chain Name       Status   Latency    Block #      URL
------------------------------------------------------------------------------------------
1          Ethereum         OK       257.8 ms   25793557     https://ethereum-rpc.publicnode.com
1          Ethereum         OK       310.2 ms   25793557     https://eth.drpc.org
1          Ethereum         OK       885.5 ms   25793555     https://1rpc.io/eth
8453       Base             OK       338.8 ms   50203075     https://base-rpc.publicnode.com
8453       Base             OK       429.9 ms   50203075     https://mainnet.base.org
42161      Arbitrum One     OK       417.5 ms   496380244    https://arbitrum-one-rpc.publicnode.com
...
------------------------------------------------------------------------------------------
✅ Total: 85 | Healthy: 78 | Failed: 7
```

### 3. 启动服务与全自动多节点负载均衡
直接启动 EVMEventLake：
- 启动时自动将 `config/rpc_endpoints.json` 中所有节点导入数据库；
- 服务端后台巡检 Worker 以 **20 个并发工作协程** 持续监控所有节点的健康度；
- 在请求区块和日志时，系统自动在每条链的几十个节点中优先选取**低失败率、高权重、低延迟**的最佳节点，某个节点限流或宕机时自动秒级无感切换到下一个可用节点。
