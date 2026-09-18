# TASK-013: 收敛与重构 Docker Compose 编排体系（默认预编译二进制构建，独立源码构建并清理冗余文件）

## 1. 任务背景与目标

为满足“本地拉取代码后直接使用编译好的二进制快速构建容器（无需在本地重新进行耗时的 Rust 编译）”的需求：
1. 将根目录 `docker-compose.yml` 调整为基于 `Dockerfile.prebuilt` 构建，构建参数指向 `deploy/prebuilt/eventlake`。
2. 明确保留 `docker-compose.source.yml` 专门用于 Rust 源码本地多阶段编译。
3. 彻底删除历史冗余文件 `docker-compose.prebuilt.yml` 与 `docker-compose.prebuilt.cn.yml`。
4. 调整 `.gitignore` 规则，允许版本库追踪 `deploy/prebuilt/eventlake`，确保在新机器拉取（git pull / clone）代码后即可直接获得二进制并启动。
5. 同步更新所有相关部署文档与指引。

---

## 2. 变更文件清单

- **新建**:
  - `docs/AI/tasks/TASK-013.md`
- **修改**:
  - `docker-compose.yml`
  - `.gitignore`
  - `deploy/prebuilt/README.md`
  - `docs/DEPLOYMENT.md`
  - `docs/BUILD.md`
  - `docs/USAGE.md`
  - `docs/AI/TASK_INDEX.md`
  - `docs/AI/SESSION_STATE.md`
- **删除**:
  - `docker-compose.prebuilt.yml`
  - `docker-compose.prebuilt.cn.yml`

---

## 3. 验收标准 (Acceptance Criteria)

1. [x] `docker-compose.yml` 构建指令指向 `Dockerfile.prebuilt`，无需 Rust 编译即可极速构建镜像。
2. [x] `docker-compose.source.yml` 保持 `Dockerfile` 源码多阶段编译。
3. [x] `docker-compose.prebuilt.yml` 与 `docker-compose.prebuilt.cn.yml` 已被删除。
4. [x] `deploy/prebuilt/eventlake` 未被 `.gitignore` 忽略，拉到本地即有二进制。
5. [x] `docker compose config` 对保留的 Compose 文件校验全部通过。
