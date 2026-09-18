# 任务索引表 (TASK INDEX)

本文档是项目中所有任务（Task）的唯一索引与生命周期看板。

---

## 1. 任务流转规范

任务状态只能按以下规则变化：

```text
TODO -> IN_PROGRESS -> REVIEW -> DONE
                    \-> BLOCKED
```

- **TODO**：已定义且依赖明确，尚未开始。
- **IN_PROGRESS**：当前会话（Session）正在执行（**一次最多一个**）。
- **REVIEW**：代码与文档已完成，等待验证或人工审查。
- **DONE**：验收标准全部满足，测试实际运行通过，文档与交接已更新。
- **BLOCKED**：缺少必要外部条件或权限，已明确记录阻塞原因。

---

## 2. 当前任务看板

### 活跃任务 (In Progress)
- （暂无活跃任务）

### 待规划 / 待执行 (TODO)
- （暂无待规划任务）

### 已完成任务 (DONE)
- **TASK-015**: [ClickHouse 系统日志轻量化抑制与写入碎片（Parts）合并防堵调优](file:///ssd0/git/EVMEventLake/docs/AI/tasks/TASK-015.md)
- **TASK-014**: [收敛合并为单一 Dockerfile（基于 Target 支持 prebuilt 与 source 模式），清理残存构建文件并恢复 Git 忽略规则](file:///ssd0/git/EVMEventLake/docs/AI/tasks/TASK-014.md)
- **TASK-013**: [收敛与重构 Docker Compose 编排体系（默认预编译二进制构建，独立源码构建并清理冗余文件）](file:///ssd0/git/EVMEventLake/docs/AI/tasks/TASK-013.md)
- **TASK-012**: [建设 GitHub Actions 手动触发发布流水线 (Release 二进制 + GHCR 预编译镜像) 与极简部署体系](file:///ssd0/git/EVMEventLake/docs/AI/tasks/TASK-012.md)
- **TASK-011**: [清理冗余 Dockerfile 与 Compose 配置，统一构建与部署流水线至纯净 SQLite + ClickHouse](file:///ssd0/git/EVMEventLake/docs/AI/tasks/TASK-011.md)
- **TASK-010**: [构建 SQLite + ClickHouse 统一备份恢复体系与 S3 增量运维脚本](file:///ssd0/git/EVMEventLake/docs/AI/tasks/TASK-010.md)
- **TASK-009**: [存储层全面迁移至 SQLite 元数据引擎 + ClickHouse 唯一数据湖](file:///ssd0/git/EVMEventLake/docs/AI/tasks/TASK-009.md)
- **TASK-008**: [压平重构 PostgreSQL 数据库迁移 (Squash Migrations) 并清理废弃表残留](file:///ssd0/git/EVMEventLake/docs/AI/tasks/TASK-008.md)
- **TASK-007**: [移除 SSH_TUNNEL 隧道及容器代理相关实现与配置](file:///ssd0/git/EVMEventLake/docs/AI/tasks/TASK-007.md)
- **TASK-006**: [审计优化实施（清理残留废弃表查询、轻量化Reorg流程、补齐交易收据API与收敛输入校验）](file:///ssd0/git/EVMEventLake/docs/AI/tasks/TASK-006.md)
- **TASK-005**: [核心日志采集流水线并发化改造与多链/多订阅异步隔离](file:///ssd0/git/EVMEventLake/docs/AI/tasks/TASK-005.md)
- **TASK-004**: [实现 RPC 节点池阶梯式退避冷却 (1m->5m->24h) 与内存化路由缓存](file:///ssd0/git/EVMEventLake/docs/AI/tasks/TASK-004.md)
- **TASK-003**: [精简 ClickHouse 存储与 Reorg 流程（清理废表定义、消除 Reorg 4倍写入放大与空表 DDL 调度）](file:///ssd0/git/EVMEventLake/docs/AI/tasks/TASK-003.md)
- **TASK-002**: [剥离并彻底删除遗留解码 (Decoder)、ABI 注册表 (ABI Registry) 与 Explorers 模块](file:///ssd0/git/EVMEventLake/docs/AI/tasks/TASK-002.md)
- **TASK-001**: [扩展区块交易采集模块以支持交易收据 (Receipts) 与 L2 燃气指标](file:///ssd0/git/EVMEventLake/docs/AI/tasks/TASK-001.md)
- **TASK-000**: AI Agent 文档架构与格式规范初始化 (初始化完成，建立规范体系)

---

## 3. 任务定义模板与规范

新增任务必须在 [`docs/AI/tasks/`](file:///ssd0/git/EVMEventLake/docs/AI/tasks/) 目录下创建独立文件，并遵循标准模板：
- 模板文件：[`docs/AI/tasks/TASK-TEMPLATE.md`](file:///ssd0/git/EVMEventLake/docs/AI/tasks/TASK-TEMPLATE.md)
- 命名规则：`TASK-xxx.md`（三位数字递增，例如 `TASK-001.md`）
- 规模要求：单一明确目标，预估 30~90 分钟，修改文件不超过 5 个实现文件与 3 个测试文件，具备明确的验收标准与可执行验证命令。
