#!/usr/bin/env bash
set -euo pipefail

# EVMEventLake 预编译二进制一键安装/更新脚本
# 用法:
#   curl -sSL https://raw.githubusercontent.com/xzsean666/EVMEventLake/main/scripts/install.sh | bash
#   或者带参数:
#   bash install.sh --version v0.1.1 --cn

REPO="xzsean666/EVMEventLake"
INSTALL_DIR="/usr/local/bin"
TARGET_VERSION="latest"
USE_CN_PROXY=false
USE_SUDO=true

print_usage() {
    cat <<EOF
Usage: install.sh [OPTIONS]

Options:
  -v, --version <TAG>     指定安装版本 (例如 v0.1.1, 默认: latest)
  -d, --dir <DIR>         指定安装二进制目录 (默认: /usr/local/bin)
  --cn                    使用中国大陆加速镜像 (ghproxy.net)
  --no-sudo               不使用 sudo 进行安装
  -h, --help              显示帮助信息
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        -v|--version)
            TARGET_VERSION="$2"
            shift 2
            ;;
        -d|--dir)
            INSTALL_DIR="$2"
            shift 2
            ;;
        --cn)
            USE_CN_PROXY=true
            shift
            ;;
        --no-sudo)
            USE_SUDO=false
            shift
            ;;
        -h|--help)
            print_usage
            exit 0
            ;;
        *)
            echo "Unknown argument: $1" >&2
            print_usage
            exit 1
            ;;
    esac
done

# 架构与系统检测
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

if [[ "$OS" != "linux" ]]; then
    echo "错误: 目前官方预编译二进制仅支持 Linux 系统 (当前检测到: $OS)" >&2
    exit 1
fi

case "$ARCH" in
    x86_64|amd64)
        ARCH_TAG="amd64"
        ;;
    *)
        echo "错误: 暂不支持架构 $ARCH，目前仅提供 amd64 (x86_64) 二进制" >&2
        exit 1
        ;;
esac

# 检查依赖工具
for cmd in curl tar; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        echo "错误: 系统未安装必要工具: $cmd" >&2
        exit 1
    fi
done

# 解析版本号
PROXY_PREFIX=""
if [[ "$USE_CN_PROXY" == "true" ]]; then
    PROXY_PREFIX="https://ghproxy.net/"
fi

if [[ "$TARGET_VERSION" == "latest" ]]; then
    echo "==> 正在查询 GitHub 最新版本..."
    LATEST_API_URL="${PROXY_PREFIX}https://api.github.com/repos/${REPO}/releases/latest"
    
    # 尝试从 GitHub API 获取 tag_name
    TAG_NAME=$(curl -sSL "$LATEST_API_URL" | grep -o '"tag_name": *"[^"]*"' | head -n 1 | cut -d '"' -f 4 || true)
    
    if [[ -z "$TAG_NAME" ]]; then
        # 兜底：通过跳转 URL 获取
        TAG_NAME=$(curl -sSL -I -o /dev/null -w "%{url_effective}" "https://github.com/${REPO}/releases/latest" | sed 's#.*/tag/##')
    fi

    if [[ -z "$TAG_NAME" || "$TAG_NAME" == "latest" ]]; then
        echo "错误: 无法获取最新 Release 版本号，请检查网络或使用 -v 参数手动指定版本。" >&2
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

echo "==> 即将安装 EVMEventLake"
echo "    版本: ${TAG_NAME}"
echo "    架构: ${ARCH_TAG}"
echo "    目标路径: ${INSTALL_DIR}/eventlake"
echo "    下载地址: ${DOWNLOAD_URL}"

TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

echo "==> 正在下载预编译压缩包..."
curl -fSL "$DOWNLOAD_URL" -o "$TMP_DIR/$ARCHIVE_NAME"

echo "==> 正在解压..."
tar -xzf "$TMP_DIR/$ARCHIVE_NAME" -C "$TMP_DIR"

if [[ ! -f "$TMP_DIR/eventlake" ]]; then
    echo "错误: 压缩包内未找到 eventlake 可执行文件" >&2
    exit 1
fi

SUDO_CMD=""
if [[ "$USE_SUDO" == "true" && $EUID -ne 0 ]]; then
    if command -v sudo >/dev/null 2>&1; then
        SUDO_CMD="sudo"
    else
        echo "提示: 当前非 root 且无 sudo 命令，尝试直接安装到目标目录..."
    fi
fi

echo "==> 正在安装到 ${INSTALL_DIR}..."
$SUDO_CMD mkdir -p "$INSTALL_DIR"
$SUDO_CMD install -m 0755 "$TMP_DIR/eventlake" "$INSTALL_DIR/eventlake"

echo "==> 验证安装..."
if "$INSTALL_DIR/eventlake" --help >/dev/null 2>&1 || true; then
    echo "✓ EVMEventLake ${TAG_NAME} 安装成功！"
    echo "  运行文件: ${INSTALL_DIR}/eventlake"
fi
