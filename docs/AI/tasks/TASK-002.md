# TASK-002: 剥离并彻底删除遗留解码 (Decoder)、ABI 注册表 (ABI Registry) 与 Explorers 模块

## Objective
全面践行 Raw Event Lake 架构定位，彻底删除未使用的后台 Decoder 代码及其关联的 ABI Registry、Explorers 接口，从依赖项中移除 `alloy-dyn-abi` 和 `alloy-json-abi`，从全局状态中移除 `abi_cache`，并将搜索接口统一收敛至高性能原始日志检索 `/api/raw-logs/search`，使系统瘦身 ~25% 并减少编译与运行负担。

## Scope
- 包含：
  - 删除 `src/decoder/` 目录及其源码文件。
  - 删除 `src/abi_registry/` 目录及其源码文件。
  - 删除 `src/explorers/` 目录及其源码文件。
  - 精简 `src/indexing/mod.rs`（移除 `index_decoded_event` 等未引用函数，仅保留分区管理模块导出）。
  - 清理 `src/lib.rs`、`src/api/routes.rs`（移除废弃路由与 OpenAPI 挂载）。
  - 清理 `src/app/application_state.rs`（移除 `abi_cache` 字段与初始化）。
  - 清理 `Cargo.toml`（移除未使用的 `alloy-dyn-abi` 和 `alloy-json-abi`）。
  - 精简 `src/search/mod.rs`（废弃/移除旧版基于解码事件的 `/api/search`，保留 `/api/raw-logs/search` 作为主检索接口）。
  - 清理 `src/dashboard/mod.rs`（不再查询废弃的 `eventlake_decoded_events` 表）。
  - 调整相关测试文件（移除 `tests/abi_registry_tests.rs`，适配 `tests/search_dsl_tests.rs` 与 `tests/e2e_real_database_tests.rs`）。
- 不包含：
  - ClickHouse 废弃表清理与 DDL 瘦身（由后续 TASK-003 处理）。
  - RPC 节点冷却机制（由后续 TASK-004 处理）。
  - 采集器并发改造（由后续 TASK-005 处理）。

## Allowed Files
- `Cargo.toml`
- `src/lib.rs`
- `src/api/routes.rs`
- `src/app/application_state.rs`
- `src/indexing/mod.rs`
- `src/search/mod.rs`
- `src/dashboard/mod.rs`
- `tests/search_dsl_tests.rs`
- `tests/e2e_real_database_tests.rs`
- `docs/AI/TASK_INDEX.md`
- `docs/AI/tasks/TASK-002.md`
- `docs/AI/SESSION_STATE.md`

## Dependencies
- 前置任务：TASK-001 (DONE)
- 外部依赖：None

## Inputs and Outputs
- **Inputs**: 现有的 Rust 单体项目源码。
- **Outputs**: 精简后的纯粹 Raw Lake 核心代码库，依赖精简，无僵尸解码与未生效接口。

## Acceptance Criteria
- [x] `src/decoder/`、`src/abi_registry/`、`src/explorers/` 已彻底删除。
- [x] `Cargo.toml` 中成功移除 `alloy-dyn-abi` 与 `alloy-json-abi`，项目能够正常下载与编译。
- [x] `src/app/application_state.rs` 不再包含 `abi_cache`。
- [x] `src/api/routes.rs` 成功移除废弃的 ABI、Explorer 与旧版 Search 路由。
- [x] `/api/raw-logs/search` 保持正常功能与 OpenAPI 文档完整。
- [x] `cargo check --locked --features clickhouse` 检查无错误。
- [x] `cargo check --locked --all-targets` 检查无错误。
- [x] 现有测试套件通过。

## Verification Commands
```bash
# 1. 默认特性编译检查
cargo check --ignore-rust-version --all-targets

# 2. ClickHouse 特性编译检查
cargo check --ignore-rust-version --features clickhouse --all-targets

# 3. 运行核心单元与集成测试
cargo test --ignore-rust-version --test search_dsl_tests
cargo test --ignore-rust-version --test block_transaction_test
cargo test --ignore-rust-version --features clickhouse --lib
```

## Risks and Assumptions
- 风险：移除 `/api/abis` 和 `/api/explorer/*` 会影响原本依赖这些 API 的下游。但经确认当前架构中新采集数据本就不写入这些表，该移除是符合系统定位的必要破坏性变更。
- 假设：`alloy-primitives` 仍被保留供数值与哈希解析使用。

## Status
DONE
