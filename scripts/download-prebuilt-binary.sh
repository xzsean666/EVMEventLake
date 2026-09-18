#!/usr/bin/env bash
set -euo pipefail

# EVMEventLake 预编译二进制下载脚本 (Docker 构建上下文准备)
# 用法:
#   ./scripts/download-prebuilt-binary.sh
#   ./scripts/download-prebuilt-binary.sh --cn
#   ./scripts/download-prebuilt-binary.sh --version v0.1.1 --cn

REPO="${EVENTLAKE_GITHUB_REPO:-xzsean666/EVMEventLake}"
OUTPUT_PATH="${EVENTLAKE_PREBUILT_BINARY:-deploy/prebuilt/eventlake}"
TARGET_VERSION="${EVENTLAKE_VERSION:-latest}"
USE_CN_PROXY=false

print_usage() {
    cat <<EOF
Usage: download-prebuilt-binary.sh [OPTIONS]

下载 GitHub Release 官方编译的 Linux 预编译二进制到本地 Docker 构建上下文 (deploy/prebuilt/eventlake)。

Options:
  -v, --version <TAG>     指定下载版本 (如 v0.1.1, 默认: latest)
  -o, --output <PATH>     指定输出路径 (默认: deploy/prebuilt/eventlake)
  -r, --repo <REPO>       指定 GitHub 仓库 (默认: xzsean666/EVMEventLake)
  --cn                    使用中国大陆加速代理 (ghproxy.net)
  -h, --help              显示帮助信息
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        -v|--version)
            TARGET_VERSION="$2"
            shift 2
            ;;
        -o|--output)
            OUTPUT_PATH="$2"
            shift 2
            ;;
        -r|--repo)
            REPO="$2"
            shift 2
            ;;
        --cn)
            USE_CN_PROXY=true
            shift
            ;;
        -h|--help)
            print_usage
            exit 0
            ;;
        *)
            echo "错误: 未知参数 $1" >&2
            print_usage
            exit 1
            ;;
    esac
done

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

# 检查基础工具
for cmd in curl tar; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        echo "错误: 缺少必要命令: $cmd" >&2
        exit 1
    fi
done

# 架构判定
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

if [[ "$OS" != "linux" ]]; then
    echo "警告: 当前操作系统为 $OS，官方 Release 二进制仅构建了 Linux amd64 目标平台。" >&2
    echo "      下载的 Linux 二进制可供 Docker Compose Linux 容器构建直接使用。" >&2
fi

case "$ARCH" in
    x86_64|amd64)
        ARCH_TAG="amd64"
        ;;
    *)
        # 默认回退使用 amd64 供 Docker 容器
        ARCH_TAG="amd64"
        ;;
esac

PROXY_PREFIX=""
if [[ "$USE_CN_PROXY" == "true" ]]; then
    PROXY_PREFIX="https://ghproxy.net/"
fi

echo "==> 正在解析 Release 版本信息 (仓库: ${REPO})..."
if [[ "$TARGET_VERSION" == "latest" ]]; then
    LATEST_API_URL="${PROXY_PREFIX}https://api.github.com/repos/${REPO}/releases/latest"
    TAG_NAME=$(curl -sSL "$LATEST_API_URL" | grep -o '"tag_name": *"[^"]*"' | head -n 1 | cut -d '"' -f 4 || true)
    
    if [[ -z "$TAG_NAME" ]]; then
        TAG_NAME=$(curl -sSL -I -o /dev/null -w "%{url_effective}" "https://github.com/${REPO}/releases/latest" | sed 's#.*/tag/##')
    fi

    if [[ -z "$TAG_NAME" || "$TAG_NAME" == "latest" ]]; then
        echo "错误: 无法从 GitHub 获取 ${REPO} 最新 Release 标签。" >&2
        echo "      如果该仓库尚未发布 Release，请先执行 scripts/build-prebuilt-binary.sh 本地编译生成二进制。" >&2
        exit 1
    fi
else
    TAG_NAME="$TARGET_VERSION"
    if [[ "$TAG_NAME" != v* ]]; then
        TAG_NAME="v${TAG_NAME}"
    fi
fi

ARCHIVE_NAME="eventlake-${TAG_NAME}-linux-${ARCH_TAG}.tar.gz"
DOWNLOAD_URL="${PROXY_PREFIX}https://github.com/${REPO}/releases/download/${TAG_NAME}/${ARCHIVE_NAME}"

echo "==> 即将下载 EVMEventLake 预编译二进制"
echo "    版本: ${TAG_NAME}"
echo "    架构: ${ARCH_TAG}"
echo "    下载地址: ${DOWNLOAD_URL}"
echo "    目标文件: ${OUTPUT_PATH}"

TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

echo "==> 开始下载压缩包..."
if ! curl -fSL "$DOWNLOAD_URL" -o "$TMP_DIR/$ARCHIVE_NAME"; then
    echo "错误: 下载失败，请检查网络或版本 ${TAG_NAME} 是否存在 Release 资产。" >&2
    exit 1
fi

echo "==> 解压提取二进制..."
tar -xzf "$TMP_DIR/$ARCHIVE_NAME" -C "$TMP_DIR"

if [[ -f "$TMP_DIR/eventlake" ]]; then
    BIN_SRC="$TMP_DIR/eventlake"
elif [[ -f "$TMP_DIR/target/release/eventlake" ]]; then
    BIN_SRC="$TMP_DIR/target/release/eventlake"
else
    echo "错误: 压缩包中未找到 eventlake 可执行文件。" >&2
    exit 1
fi

mkdir -p "$(dirname "$OUTPUT_PATH")"
install -m 0755 "$BIN_SRC" "$OUTPUT_PATH"

echo "✓ 预编译二进制下载成功: ${OUTPUT_PATH}"
echo "  大小: $(du -h "$OUTPUT_PATH" | cut -f1)"
echo "  现在可以直接运行以下命令构建启动容器（零 Rust 编译）："
echo "    docker compose up -d --build"
