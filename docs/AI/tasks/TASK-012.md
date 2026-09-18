# TASK-012: 建设 GitHub Actions 手动触发发布流水线与零编译极简部署体系

## 1. 任务背景与目标

当前仓库的预编译部署模式（`scripts/build-prebuilt-binary.sh` + `docker-compose.prebuilt.yml`）仍需在目标服务器本地执行耗时且占用大量资源的 `cargo build --release` 和本地 `docker build`。

本任务目标：
1. **GitHub Actions 云端流水线**：配置手动触发（`workflow_dispatch`）的构建工作流，触发时自动计算并更新版本号（patch/minor/major/自定义），打 tag 并推送到 GitHub。
2. **GitHub Release 二进制包发布**：云端编译 Linux x86_64 release 二进制文件，打包并生成 SHA256 校验和，自动发布到 GitHub Release。
3. **预编译 Docker 镜像与 latest 编排**：流水线将预编译镜像推送到 GitHub Packages (GHCR)，打上版本号和 `latest` 标签；改造 `docker-compose.yml` 默认拉取 `latest`，实现服务器端 0 构建秒级启动。
4. **服务器端一键安装脚本与 Systemd 守护**：提供 `scripts/install.sh` 与 `deploy/systemd/eventlake.service`，满足不依赖 Docker 的纯二进制极简部署需求。

---

## 2. 变更文件清单

- **新建**:
  - `.github/workflows/release.yml`
  - `scripts/install.sh`
  - `deploy/systemd/eventlake.service`
  - `docs/AI/tasks/TASK-012.md`
- **调整**:
  - `docker-compose.yml`
  - `docker-compose.prebuilt.yml`
  - `docker-compose.prebuilt.cn.yml`
  - `docs/DEPLOYMENT.md`
  - `docs/BUILD.md`
  - `docs/AI/TASK_INDEX.md`
  - `docs/AI/SESSION_STATE.md`

---

## 3. 验收标准 (Acceptance Criteria)

1. [x] `.github/workflows/release.yml` 语法合规，支持 `workflow_dispatch` 手动输入或选择 bump 类型。
2. [x] 工作流完成版本号在 `Cargo.toml` 中的更新、git tag 签名推送以及 GitHub Release 发布。
3. [x] 工作流构建 Docker 镜像并推送至 `ghcr.io/xzsean666/evmeventlake:latest` 和 `ghcr.io/xzsean666/evmeventlake:<version>`。
4. [x] Compose 文件默认采用预编译远程镜像，支持 `docker compose pull && docker compose up -d` 零本地编译启动。
5. [x] `scripts/install.sh` 具备架构检测、Release 二进制下载与国内加速代理支持。
6. [x] 所有测试与语法检查（`bash -n`、`docker compose config`）均通过。
