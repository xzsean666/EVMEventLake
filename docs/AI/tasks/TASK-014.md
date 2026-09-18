# TASK-014: 收敛合并为单一 Dockerfile（基于 Target 支持 prebuilt 与 source 模式），清理残存构建文件并恢复 Git 忽略规则

## 1. 任务背景与目标

当前仓库中仍存在 `Dockerfile.prebuilt` 与 `Dockerfile.prebuilt.cn` 残存文件，且前次变更使 19MB 二进制文件暴露于 Git 追踪中，不符合规范工程实践：
1. **统一 Dockerfile**：利用 Docker BuildKit 的多阶段构建目标（Multi-stage Target）机制，合并为一个唯一的 `Dockerfile`，定义 `runtime-base`、`prebuilt`（秒级打包外部二进制）和 `source`（Rust 完整多阶段编译）三个阶段。
2. **清理冗余残存文件**：删除 `Dockerfile.prebuilt` 与 `Dockerfile.prebuilt.cn`。
3. **适配编排与流水线**：
   - `docker-compose.yml` 配置 `target: prebuilt`。
   - `docker-compose.source.yml` 配置 `target: source`。
   - `.github/workflows/release.yml` 配置 `--target prebuilt`。
4. **规范二进制资产托管**：恢复 `.gitignore` 对 `/deploy/prebuilt/*` 的忽略规则，让构建的二进制资产统一通过 GitHub Releases 与 GHCR 容器镜像交付，保持 Git 仓库纯净轻量。

---

## 2. 变更文件清单

- **新建**:
  - `docs/AI/tasks/TASK-014.md`
- **修改**:
  - `Dockerfile`
  - `docker-compose.yml`
  - `docker-compose.source.yml`
  - `.github/workflows/release.yml`
  - `.gitignore`
  - `docs/DEPLOYMENT.md`
  - `docs/BUILD.md`
  - `docs/AI/TASK_INDEX.md`
  - `docs/AI/SESSION_STATE.md`
- **删除**:
  - `Dockerfile.prebuilt`
  - `Dockerfile.prebuilt.cn`

---

## 3. 验收标准 (Acceptance Criteria)

1. [x] 仓库仅保留根目录唯一 `Dockerfile`，`Dockerfile.prebuilt` 与 `Dockerfile.prebuilt.cn` 已删除。
2. [x] `docker-compose.yml`（prebuilt 模式）与 `docker-compose.source.yml`（source 模式）指向单一 `Dockerfile` 对应 target。
3. [x] `.github/workflows/release.yml` 使用 `--target prebuilt` 构建镜像。
4. [x] `.gitignore` 恢复忽略 `/deploy/prebuilt/*`，保持 Git 纯净。
5. [x] `docker compose config` 与 `docker build --target prebuilt` 验证通过。
