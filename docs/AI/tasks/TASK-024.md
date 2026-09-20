# TASK-024: lakectl 支持先登录后操作 (login/logout/whoami) 与自定义远程端口

## Objective
完善 `lakectl` 的 3 种远程与本地连接架构（公网 URL 暴露直连、SSH 隧道安全穿透内部端口、本地直连），增加 `--remote-port` 自定义支持，并实现“先登录再操作”的配置上下文持久化机制（`lakectl login / logout / whoami`），使用户只需一次登录，即可在后续所有命令中免输入连接与凭据参数。

## Scope
- 包含：
  - 在 `scripts/lakectl` 中增加 `--remote-port <port>` 选项（支持自定义远程目标端口，默认 8080）。
  - 实现凭据持久化：启动时自动读取 `~/.lakectl/config`（若存在且未在命令行显式覆盖）。
  - 新增 `lakectl login`：接收参数并保存当前连接上下文（URL、Token、SSH 目标、SSH 端口、密钥文件、远端服务端口）到 `~/.lakectl/config`（600 安全权限）。
  - 新增 `lakectl whoami`：输出当前生效的连接模式（直接 HTTP 还是 SSH 穿透）、目标地址、端口及凭据状态。
  - 新增 `lakectl logout`：清除当前保存的连接凭据。
  - 更新 `docs/USAGE.md`，增加 3 种连接模式详解与登录免密操作指引。
- 不包含：
  - 修改服务端核心业务代码。

## Allowed Files
- `scripts/lakectl`
- `docs/USAGE.md`
- `docs/AI/tasks/TASK-024.md`
- `docs/AI/TASK_INDEX.md`
- `docs/AI/SESSION_STATE.md`

## Dependencies
- 前置任务：TASK-023 (已完成)

## Inputs and Outputs
- **Inputs**:
  - 用户提出的 3 种连接模式分类及“先登录再操作”的诉求
- **Outputs**:
  - `scripts/lakectl` 扩展 `login/logout/whoami` 及 `--remote-port`
  - 更新后的文档与测试

## Acceptance Criteria
- [x] 支持 3 种登录连接模式（公网 URL 直连、SSH 内部端口穿透、本地回环直连）。
- [x] `lakectl login` 成功将配置保存至 `~/.lakectl/config` 并设置 600 安全权限。
- [x] 登录成功后，后续直接执行 `lakectl top`、`lakectl status` 等命令无需再重复输入连接参数。
- [x] `lakectl whoami` 准确展示当前连接目标、穿透模式与认证状态。
- [x] `lakectl logout` 成功清理保存的配置文件。
- [x] 脚本语法检查退出码 0 (`bash -n scripts/lakectl`)。

## Verification Commands
```bash
bash -n scripts/lakectl
./scripts/lakectl --help
./scripts/lakectl whoami
./scripts/lakectl logout
```

## Verification Results
- `bash -n scripts/lakectl`：语法检查通过，退出码 0。
- `./scripts/lakectl --help`：完整展示 `login`、`whoami`、`logout` 与 `--remote-port`。
- `./scripts/lakectl whoami`：准确输出当前连接模式、目标、端口与连通性检测结果。
- `./scripts/lakectl logout`：正确清理本地持久化凭据。
- 登录连通性自测：`lakectl login` 在测试失败时拦截并给出诊断指引，测试通过时写入 `~/.lakectl/config` (600 权限)。
- 文档同步：`docs/USAGE.md` 已补充 3 种连接模式与登录使用指南。
- 任务标记完成。
