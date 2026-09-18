# Prebuilt Binary (预编译构建上下文)

本目录用于存放 Linux x86_64 预编译二进制文件，作为 `Dockerfile`（阶段 `target: prebuilt`）和 `docker-compose.yml` 的免编译秒级构建上下文输入。

> [!NOTE]
> 为避免大文件污染 Git 历史，本目录下的二进制文件已被 `.gitignore` 全局忽略，不随 Git 仓库提交。

### 1. 从 GitHub Release 一键下载官方二进制（推荐）：

```bash
# 默认下载最新版本
./scripts/download-prebuilt-binary.sh

# 中国大陆加速下载
./scripts/download-prebuilt-binary.sh --cn

# 指定下载特定 Release 版本
./scripts/download-prebuilt-binary.sh --version v0.1.1 --cn
```

### 2. 本地重新编译生成二进制：

若本地安装了 Rust 工具链，可直接本地编译生成：

```bash
./scripts/build-prebuilt-binary.sh
```

### 3. Docker 构建行为：

当运行 `docker compose up -d --build` 时：
- 若本目录下存在 `eventlake` 二进制，Docker 自动秒级复用；
- 若本目录下不存在 `eventlake`，Docker 构建过程将自动通过 `curl` 从 GitHub Releases 拉取最新预编译包并解压安装。

