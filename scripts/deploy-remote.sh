#!/usr/bin/env bash
set -euo pipefail

# ==============================================================================
# EVMEventLake 远程一键部署自动化脚本 (deploy-remote.sh)
#
# 功能说明:
#   通过 SSH 将本地项目同步至目标远程服务器，并基于 Docker Compose 进行秒级
#   预编译构建与平滑重启部署。自动保持宿主机持久化数据（SQLite 与 ClickHouse）不丢失。
#
# 用法示例:
#   1. 命令行参数模式:
#      ./scripts/deploy-remote.sh -s root@1.2.3.4 -d /opt/eventlake -e .env.prod
#
#   2. 带端口与密钥模式:
#      ./scripts/deploy-remote.sh -s ubuntu@1.2.3.4 -p 2222 -i ~/.ssh/id_rsa -d /opt/eventlake -e .env.prod --cn
#
#   3. 交互式引导模式（直接运行，脚本会提示输入各项参数）:
#      ./scripts/deploy-remote.sh
# ==============================================================================

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

log_info() { echo -e "${BLUE}[INFO]${NC} $*"; }
log_succ() { echo -e "${GREEN}[SUCCESS]${NC} $*"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $*"; }
log_err()  { echo -e "${RED}[ERROR]${NC} $*" >&2; }

print_usage() {
    cat <<EOF
Usage: $(basename "$0") [OPTIONS]

一键远程自动化部署 EVMEventLake 到目标 Linux 服务器。

选项:
  -s, --ssh <TARGET>       SSH 连接目标 (格式: user@host 或 host，如 root@192.168.1.100)
  -p, --port <PORT>        SSH 连接端口 (默认: 22)
  -i, --identity <KEY>     SSH 私钥路径 (例如: ~/.ssh/id_rsa，可选)
  -d, --dir <DIR>          远端目标部署目录 (例如: /opt/eventlake)
  -e, --env <FILE>         指定的本地 .env 配置文件路径 (例如: .env.production 或 .env)
  --cn                     启用中国大陆网络加速代理 (下载构建提速)
  --source                 强制使用源码全量重新编译构建 (docker-compose.source.yml)
  --dry-run                仅测试 SSH 连接与环境，不执行代码同步和容器重启
  -h, --help               显示此帮助信息

示例:
  $(basename "$0") -s root@47.100.1.2 -d /opt/eventlake -e ./my-prod.env
  $(basename "$0") -s deploy@example.com -p 2202 -i ~/.ssh/deploy_key -d /data/eventlake -e .env --cn
EOF
}

# 默认变量
SSH_TARGET=""
SSH_PORT=22
SSH_KEY=""
REMOTE_DIR=""
LOCAL_ENV=""
USE_CN_PROXY=false
USE_SOURCE_BUILD=false
DRY_RUN=false

# 解析命令行参数
while [[ $# -gt 0 ]]; do
    case "$1" in
        -s|--ssh)
            SSH_TARGET="$2"
            shift 2
            ;;
        -p|--port)
            SSH_PORT="$2"
            shift 2
            ;;
        -i|--identity)
            SSH_KEY="$2"
            shift 2
            ;;
        -d|--dir)
            REMOTE_DIR="$2"
            shift 2
            ;;
        -e|--env)
            LOCAL_ENV="$2"
            shift 2
            ;;
        --cn)
            USE_CN_PROXY=true
            shift
            ;;
        --source)
            USE_SOURCE_BUILD=true
            shift
            ;;
        --dry-run)
            DRY_RUN=true
            shift
            ;;
        -h|--help)
            print_usage
            exit 0
            ;;
        *)
            log_err "未知参数: $1"
            print_usage
            exit 1
            ;;
    esac
done

# 检查是否有必要参数缺失且在交互式终端中，才启动交互式引导
INTERACTIVE_MODE=false
if [ -t 0 ] && { [ -z "$SSH_TARGET" ] || [ -z "$REMOTE_DIR" ] || [ -z "$LOCAL_ENV" ]; }; then
    INTERACTIVE_MODE=true
    echo -e "${CYAN}=== EVMEventLake 远程自动化部署向导 ===${NC}"
    if [ -z "$SSH_TARGET" ]; then
        read -r -p "请输入 SSH 目标地址 (例如 root@1.2.3.4): " input_target
        SSH_TARGET="${input_target:-}"
    fi

    if [ -z "$REMOTE_DIR" ]; then
        read -r -p "请输入远端部署目录 [默认: /opt/eventlake]: " input_dir
        REMOTE_DIR="${input_dir:-/opt/eventlake}"
    fi

    if [ -z "$LOCAL_ENV" ]; then
        read -r -p "请输入指定的本地 .env 路径 [默认: .env]: " input_env
        LOCAL_ENV="${input_env:-.env}"
    fi

    if [ "$USE_CN_PROXY" = false ]; then
        read -r -p "目标服务器是否位于中国大陆（启用网络加速代理）? (y/N): " input_cn
        if [[ "$input_cn" =~ ^[Yy]$ ]]; then
            USE_CN_PROXY=true
        fi
    fi
fi

# 参数有效性基础校验
if [ -z "$SSH_TARGET" ]; then
    log_err "缺少必须的 SSH 目标 (-s/--ssh)。"
    print_usage
    exit 1
fi

if [ -z "$REMOTE_DIR" ]; then
    log_err "缺少必须的远端目录 (-d/--dir)。"
    print_usage
    exit 1
fi

if [ -z "$LOCAL_ENV" ]; then
    log_err "缺少指定的本地 env 配置文件 (-e/--env)。"
    print_usage
    exit 1
fi

if [ ! -f "$LOCAL_ENV" ]; then
    log_err "指定的本地 env 配置文件不存在: $LOCAL_ENV"
    exit 1
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# SSH / SCP 调用包装函数
SSH_OPTS=(-p "$SSH_PORT" -o "StrictHostKeyChecking=accept-new" -o "ConnectTimeout=15")
SCP_OPTS=(-P "$SSH_PORT" -o "StrictHostKeyChecking=accept-new" -o "ConnectTimeout=15")

if [ -n "$SSH_KEY" ]; then
    if [ ! -f "$SSH_KEY" ]; then
        log_err "指定的 SSH 私钥文件不存在: $SSH_KEY"
        exit 1
    fi
    SSH_OPTS+=(-i "$SSH_KEY")
    SCP_OPTS+=(-i "$SSH_KEY")
fi

run_ssh() {
    ssh "${SSH_OPTS[@]}" "$SSH_TARGET" "$@"
}

run_scp() {
    scp "${SCP_OPTS[@]}" "$@"
}

log_info "=================================================="
log_info "开始 EVMEventLake 远程部署"
log_info "目标服务器: $SSH_TARGET (端口: $SSH_PORT)"
log_info "远端目录:   $REMOTE_DIR"
log_info "本地配置文件: $LOCAL_ENV"
log_info "加速代理:   $USE_CN_PROXY"
log_info "构建模式:   $([ "$USE_SOURCE_BUILD" = true ] && echo "源码编译 (source)" || echo "预编译极速 (prebuilt)")"
log_info "=================================================="

# ------------------------------------------------------------------------------
# 步骤 1: 测试 SSH 连通性与远端环境探测
# ------------------------------------------------------------------------------
log_info "==> [1/5] 测试 SSH 连通性并检查远端环境..."
if ! run_ssh "echo 'SSH connected successfully' >/dev/null"; then
    log_err "无法通过 SSH 连接到目标服务器: $SSH_TARGET"
    exit 1
fi
log_succ "SSH 连通性正常。"

# 远端依赖检查 (Docker & Docker Compose)
REMOTE_CHECK_SCRIPT=$(cat << 'EOF'
set -e
if ! command -v docker >/dev/null 2>&1; then
    echo "DOCKER_NOT_FOUND"
    exit 0
fi

if docker compose version >/dev/null 2>&1; then
    echo "COMPOSE_V2"
elif command -v docker-compose >/dev/null 2>&1; then
    echo "COMPOSE_V1"
else
    echo "COMPOSE_NOT_FOUND"
    exit 0
fi
EOF
)

CHECK_RESULT=$(run_ssh "bash -s" <<< "$REMOTE_CHECK_SCRIPT")

COMPOSE_BIN=""
case "$CHECK_RESULT" in
    *"COMPOSE_V2"*)
        COMPOSE_BIN="docker compose"
        ;;
    *"COMPOSE_V1"*)
        COMPOSE_BIN="docker-compose"
        ;;
    *"DOCKER_NOT_FOUND"*)
        log_err "目标服务器上未检测到 Docker，请先在目标机器安装 Docker 后重试。"
        exit 1
        ;;
    *"COMPOSE_NOT_FOUND"*)
        log_err "目标服务器上未检测到 Docker Compose 插件或二进制，请安装 compose。"
        exit 1
        ;;
    *)
        log_warn "未能确定 Docker Compose 版本，默认尝试使用 'docker compose'。"
        COMPOSE_BIN="docker compose"
        ;;
esac

log_succ "远端 Docker 环境检测通过 (使用编排命令: $COMPOSE_BIN)。"

if [ "$DRY_RUN" = true ]; then
    log_succ "Dry-run 模式测试完成，退出。"
    exit 0
fi

# ------------------------------------------------------------------------------
# 步骤 2: 远端目录初始化与数据持久化保护
# ------------------------------------------------------------------------------
log_info "==> [2/5] 初始化远端目录结构 (保留历史数据)..."

CH_DATA_DIR=""
if [ -f "$LOCAL_ENV" ]; then
    CH_DATA_DIR=$(grep -E '^[[:space:]]*CLICKHOUSE_DATA_DIR=' "$LOCAL_ENV" 2>/dev/null | cut -d '=' -f2- | tr -d ' "\r' || echo "")
fi

REMOTE_MKDIRS="'$REMOTE_DIR' '$REMOTE_DIR/data/sqlite' '$REMOTE_DIR/data/clickhouse' '$REMOTE_DIR/logs/clickhouse' '$REMOTE_DIR/backups'"
if [ -n "$CH_DATA_DIR" ]; then
    if [[ "$CH_DATA_DIR" = /* ]]; then
        REMOTE_MKDIRS="$REMOTE_MKDIRS '$CH_DATA_DIR'"
    else
        REMOTE_MKDIRS="$REMOTE_MKDIRS '$REMOTE_DIR/$CH_DATA_DIR'"
    fi
fi

run_ssh "mkdir -p $REMOTE_MKDIRS && chmod -R 777 '$REMOTE_DIR/data/sqlite'"
log_succ "远端持久化目录已就绪。"

# ------------------------------------------------------------------------------
# 步骤 3: 增量安全同步项目工程文件
# ------------------------------------------------------------------------------
log_info "==> [3/5] 同步项目部署文件至远端..."

# 排除本地大文件、数据目录、日志和本地环境配置，但保留 .env.example
EXCLUDES=(
    --exclude='.git'
    --exclude='target'
    --exclude='target_local'
    --exclude='data'
    --exclude='logs'
    --exclude='backups'
    --include='.env.example'
    --exclude='.env'
    --exclude='.env.*'
    --exclude='*.log'
    --exclude='*.tmp'
    --exclude='.idea'
    --exclude='.vscode'
)

HAS_RSYNC=false
if command -v rsync >/dev/null 2>&1; then
    if run_ssh "command -v rsync >/dev/null 2>&1"; then
        HAS_RSYNC=true
    fi
fi

if [ "$HAS_RSYNC" = true ]; then
    SSH_RSYNC_RSH="ssh -p $SSH_PORT -o StrictHostKeyChecking=accept-new"
    if [ -n "$SSH_KEY" ]; then
        SSH_RSYNC_RSH="$SSH_RSYNC_RSH -i $SSH_KEY"
    fi

    rsync -avz --delete-after "${EXCLUDES[@]}" -e "$SSH_RSYNC_RSH" "$REPO_ROOT/" "$SSH_TARGET:$REMOTE_DIR/"
else
    log_info "本地或远端未安装 rsync，使用 tar over SSH 流式同步..."
    tar "${EXCLUDES[@]}" -czf - -C "$REPO_ROOT" . | run_ssh "tar -xzf - -C '$REMOTE_DIR'"
fi
log_succ "项目部署文件同步完成。"

# ------------------------------------------------------------------------------
# 步骤 4: 推送指定的本地 .env 文件至远端
# ------------------------------------------------------------------------------
log_info "==> [4/5] 推送指定的配置文件 ($LOCAL_ENV) 至远端..."
run_scp "$LOCAL_ENV" "$SSH_TARGET:$REMOTE_DIR/.env"
run_ssh "chmod 600 '$REMOTE_DIR/.env'"
log_succ "远端配置文件部署完成 ($REMOTE_DIR/.env，权限 600)。"

# ------------------------------------------------------------------------------
# 步骤 5: 远端启动与服务健康检查
# ------------------------------------------------------------------------------
log_info "==> [5/5] 远端构建并启动服务..."

COMPOSE_FILE="docker-compose.yml"
if [ "$USE_SOURCE_BUILD" = true ]; then
    COMPOSE_FILE="docker-compose.source.yml"
fi

DEPLOY_CMD="cd '$REMOTE_DIR' && \
    export EVENTLAKE_CN_PROXY='$USE_CN_PROXY' && \
    export EVENTLAKE_ENV_FILE='.env' && \
    $COMPOSE_BIN -f $COMPOSE_FILE up -d --build"

run_ssh "$DEPLOY_CMD"
log_succ "容器编排启动指令已发送。"

# 轮询健康检查端点 (最多等待 60 秒)
log_info "正在验证服务健康状态 (等待服务启动就绪)..."

HEALTHCHECK_REMOTE=$(cat << 'EOF'
set -e
PORT=$(grep -E '^EVENTLAKE_HTTP_PORT=' .env 2>/dev/null | cut -d '=' -f2 | tr -d ' "\r' || echo "")
if [ -z "$PORT" ]; then
    PORT=8080
fi

MAX_WAIT=60
INTERVAL=3
WAITED=0

echo "Checking health endpoint on 127.0.0.1:$PORT/health/ready ..."
while [ $WAITED -lt $MAX_WAIT ]; do
    if curl -fsS "http://127.0.0.1:$PORT/health/ready" >/dev/null 2>&1; then
        echo "HEALTHY:$PORT"
        exit 0
    fi
    sleep $INTERVAL
    WAITED=$((WAITED + INTERVAL))
done

echo "TIMEOUT:$PORT"
exit 1
EOF
)

HEALTH_OUTPUT=$(run_ssh "cd '$REMOTE_DIR' && bash -s" <<< "$HEALTHCHECK_REMOTE" || true)

if [[ "$HEALTH_OUTPUT" =~ HEALTHY:([0-9]+) ]]; then
    HTTP_PORT="${BASH_REMATCH[1]}"
    log_succ "=================================================="
    log_succ "🎉 EVMEventLake 远程部署成功并已通过健康检查！"
    log_succ "API 访问端点: 127.0.0.1:${HTTP_PORT} (本地回环保护，外部禁止直连)"
    log_succ "健康检查端点: http://127.0.0.1:${HTTP_PORT}/health/ready (本地回环)"
    log_succ "ClickHouse:   容器内网隔离 (不对外暴露端口，仅 API 容器可访问)"
    if [ -n "$CH_DATA_DIR" ]; then
        log_succ "持久化数据目录: $CH_DATA_DIR (ClickHouse) / $REMOTE_DIR/data/sqlite (SQLite)"
    else
        log_succ "持久化数据目录: $REMOTE_DIR/data (数据完整保留)"
    fi
    log_succ "=================================================="
else
    log_warn "服务已启动，但健康检查暂未就绪或超时。"
    log_warn "可运行以下命令登录服务器查看容器日志："
    log_warn "  ssh ${SSH_TARGET} 'cd $REMOTE_DIR && $COMPOSE_BIN logs -f'"
fi
