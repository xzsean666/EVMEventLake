# TASK-021: 完善部署后全流程业务使用与下游集成指南 (USAGE.md)

## Objective
针对已部署或准备部署的用户，编写全面、直观、开箱即用的《EventLake 部署后业务使用与下游集成指南》，覆盖服务健康检查、认证鉴权、链与 RPC 配置、3 大数据采集任务创建、数据消费检索（REST Search DSL 与 ClickHouse SQL 直连）、以及下游客户端开发示例。

## Scope
- 包含：
  - 更新并大幅扩充 `docs/USAGE.md`，构建结构清晰、图文表并茂的部署后操作手册。
  - 部署后第一步：健康检查、服务状态大盘 (`GET /api/dashboard`)、OpenAPI 接口文档导出。
  - 认证与安全配置（API Key 与 JWT 生成与调用）。
  - 核心业务流程 1：RPC 节点池配置（种子文件自动注入与动态 API，加权负载均衡与探活）。
  - 核心业务流程 2：数据采集任务创建（按单/批量智能合约事件采集、全链所有事件 All-events 采集、整链区块与交易全量同步）。
  - 核心业务流程 3：任务进度观察与运行控制（状态检查、暂停/恢复、异常排查）。
  - 核心数据消费：
    - 方式 A：REST API Search DSL 高级检索（各种操作符与条件组合）。
    - 方式 B：区块与交易/收据/地址流水 REST API。
    - 方式 C：ClickHouse 数据库直连查询（HTTP 8123 / Native 9000，ReplacingMergeTree FINAL 与 SQL 最佳实践）。
    - 方式 D：下游应用代码实操示例（Python 与 Node.js 代码示例）。
  - 日常运维指引：日志排查与一键热备还原。
- 不包含：
  - 修改核心业务 Rust 源码与 Docker 编排代码。

## Allowed Files
- `docs/USAGE.md`
- `docs/AI/tasks/TASK-021.md`
- `docs/AI/TASK_INDEX.md`
- `docs/AI/SESSION_STATE.md`

## Dependencies
- 前置任务：TASK-020 (远程一键部署脚本已完成)

## Inputs and Outputs
- **Inputs**:
  - 用户提出的使用指引诉求
  - 现有的 `docs/USAGE.md`、`docs/DEPLOYMENT.md`、`src/api/routes.rs` 等代码实现
- **Outputs**:
  - 完善更新后的 `docs/USAGE.md`
  - 任务追踪与状态更新文件

## Acceptance Criteria
- [x] 文档结构层次清晰，语言通俗专业，逻辑从「部署后验证」到「业务配置」再到「数据消费」层层递进。
- [x] 涵盖全部当前支持的 REST API 核心接口及其实际 cURL 示例。
- [x] 涵盖 ClickHouse SQL 直连使用方式与关键表（`raw_logs`, `blocks`, `transactions`）查询语法与防分叉优化。
- [x] 包含下游客户端编程语言（Python, Node.js）集成示例。
- [x] 杜绝任何废弃架构描述（如已被移除的 PostgreSQL 或本地 ABI 解码），严格契合 SQLite + ClickHouse 纯净架构。

## Verification Commands
```bash
# 验证文档链接与格式有效性
test -f docs/USAGE.md
git diff --stat docs/USAGE.md
```

## Verification Results
- 验证 `test -f docs/USAGE.md` 存在且完整。
- `git diff --stat docs/USAGE.md` 验证新增 579 行详细实操手册，涵盖系统全景流程图、健康检查、Dashboard、API Key、RPC 加权轮询、3类事件订阅、整链区块交易同步、Search DSL、REST API、ClickHouse SQL 直连查询、Python/Node.js 代码示例及日常灾备 FAQ。
- 任务标记完成。
