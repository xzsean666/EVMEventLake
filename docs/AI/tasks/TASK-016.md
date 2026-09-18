# TASK-016: 优化 Dockerfile 与 Docker Compose 支持 GitHub Release 二进制拉取构建并剥离 Git 大文件追踪

## Objective

优化 Dockerfile 的 `prebuilt` 构建阶段与 `docker-compose.yml` 编排配置：
1. 从 Git 索引中解绑已追踪的 19MB 预编译二进制文件（`deploy/prebuilt/eventlake`），恢复 `.gitignore` 规则，防止大文件污染版本库。
2. 增强 `Dockerfile` 的 `prebuilt` 阶段：本地存在二进制时优先直接复用（秒级构建）；本地无二进制时，自动根据版本参数（`EVENTLAKE_VERSION`）从 GitHub Releases（或加速代理 `ghproxy.net`）下载官方 release 二进制并解压安装。
3. 提供独立的 `scripts/download-prebuilt-binary.sh` 脚本，方便宿主机或一键脚本下载 release 产物至构建上下文。
4. 优化 `docker-compose.yml` build args 与 `.github/workflows/release.yml` 构建流水线，确保 Actions 发布的 Release 资产与 Docker 容器秒级构建闭环。

---

## Scope

- 仅涉及 Docker 构建配置、Compose 编排参数、下载脚本、Git 忽略规则与 CI 工作流。
- 不修改 Rust 核心业务代码与数据库迁移逻辑。

---

## Allowed Files

- `Dockerfile`
- `docker-compose.yml`
- `.gitignore`
- `.github/workflows/release.yml`
- `.env.example`
- `scripts/download-prebuilt-binary.sh`
- `deploy/prebuilt/README.md`
- `docs/DEPLOYMENT.md`
- `docs/BUILD.md`
- `docs/AI/TASK_INDEX.md`
- `docs/AI/SESSION_STATE.md`
- `docs/AI/tasks/TASK-016.md`

---

## Dependencies

- TASK-012, TASK-013, TASK-014, TASK-015 (`DONE`)

---

## Inputs and Outputs

- **输入**：
  - GitHub Actions 发布的 Release 资产包（`eventlake-v${VERSION}-linux-amd64.tar.gz`）。
  - 用户执行 `docker compose up -d --build` 或 `./scripts/download-prebuilt-binary.sh`。
- **输出**：
  - 精简纯净的 Git 仓库（无 19MB 二进制大文件）。
  - 具备自适应能力的 `prebuilt` Docker 镜像。
  - 健壮的 release 发布流水线。

---

## Acceptance Criteria

1. `deploy/prebuilt/eventlake` 从 Git 索引中彻底移除（`git ls-files deploy/prebuilt/eventlake` 输出为空）。
2. `.gitignore` 中 `/deploy/prebuilt/*` 且 `!/deploy/prebuilt/README.md` 规则生效。
3. `Dockerfile` 的 `prebuilt` 阶段支持两种模式：
   - 本地上下文存在二进制时，直接快速 `cp` 拷贝使用（耗时 < 1 秒）。
   - 本地上下文无二进制时，自动通过 `curl` 从 GitHub Releases 拉取指定版本（或 `latest`）并解压安装。
4. `docker-compose.yml` 配置经 `docker compose config` 验证 100% 合法。
5. `scripts/download-prebuilt-binary.sh` 可执行且具备中国大陆代理加速（`--cn`）支持。
6. 全量测试通过（`cargo test` 55 项全绿）。

---

## Verification Commands

```bash
git ls-files deploy/prebuilt/eventlake
docker compose --env-file .env.example -f docker-compose.yml config > /dev/null
docker build --target prebuilt -t eventlake:test .
cargo check --ignore-rust-version --all-targets
cargo test --ignore-rust-version
```

---

## Status

DONE

## Verification Results

1. **Git 瘦身与大文件解除追踪**：`git ls-files deploy/prebuilt/eventlake` 输出为空，19MB 二进制不再随 Git 提交，恢复规范 `.gitignore`。
2. **Dockerfile prebuilt 自适应构建**：
   - 切换运行时底层镜像为 `ubuntu:24.04`，完美对齐 GitHub Actions (`ubuntu-latest`) 与现代 Linux 系统编译环境的 GLIBC 2.38+ 动态链接要求。
   - 在构建上下文中存在二进制时，直接本地秒级复用；在全新克隆环境或无本地二进制时，自动从 GitHub Releases 下载预编译 tar.gz 并解压安装。
3. **Docker Compose 配置验证**：`docker compose --env-file .env.example -f docker-compose.yml config` 语法验证 100% 通过；`docker compose build eventlake` 构建成功。
4. **测试套件运行**：55 项单元测试与真实数据库集成测试全绿通过（`55 passed; 0 failed`）。
