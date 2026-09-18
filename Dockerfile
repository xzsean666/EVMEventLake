# syntax=docker/dockerfile:1

ARG BASE_IMAGE=ubuntu:24.04
ARG RUST_VERSION=1.94

# ====================================================
# 1. 基础运行时阶段 (runtime-base)
# 基于 Ubuntu 24.04 LTS (GLIBC 2.39+)，对齐 GitHub Actions (ubuntu-latest)
# 与现代 Linux 构建环境的预编译二进制依赖
# ====================================================
FROM ${BASE_IMAGE} AS runtime-base

RUN apt-get update && \
    DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends ca-certificates curl tar tzdata && \
    rm -rf /var/lib/apt/lists/* && \
    groupadd --system --gid 10001 eventlake && \
    useradd --system --uid 10001 --gid eventlake --home-dir /var/lib/eventlake eventlake && \
    mkdir -p /var/lib/eventlake && \
    chown -R eventlake:eventlake /var/lib/eventlake

ENV EVENTLAKE_HTTP_HOST=0.0.0.0 \
    EVENTLAKE_HTTP_PORT=8080

WORKDIR /var/lib/eventlake
USER eventlake:eventlake

EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
    CMD curl -fsS "http://127.0.0.1:${EVENTLAKE_HTTP_PORT:-8080}/health/ready" >/dev/null || exit 1

ENTRYPOINT ["eventlake"]

# ====================================================
# 2. 目标阶段 A: prebuilt (免本地编译秒级打包)
# 支持两种模式：
# 1) 本地优先：若构建上下文中已存在 deploy/prebuilt/eventlake，则秒级拷贝复用；
# 2) 远程拉取：若本地无二进制，自动从 GitHub Releases 下载官方预编译包并解压；
# 支持配置 EVENTLAKE_VERSION、USE_CN_PROXY (ghproxy.net) 或 EVENTLAKE_DOWNLOAD_URL。
# ====================================================
FROM runtime-base AS prebuilt

ARG GITHUB_REPO=xzsean666/EVMEventLake
ARG EVENTLAKE_VERSION=latest
ARG EVENTLAKE_DOWNLOAD_URL=""
ARG USE_CN_PROXY=false
ARG EVENTLAKE_BINARY=deploy/prebuilt/eventlake

USER root

# 拷贝 deploy/prebuilt 目录（即使二进制被 gitignore，此目录仍含 README.md，保证 COPY 永不报错）
COPY deploy/prebuilt /tmp/prebuilt

RUN set -eux; \
    if [ -f /tmp/prebuilt/eventlake ] && [ -s /tmp/prebuilt/eventlake ]; then \
        echo "==> [Prebuilt] Using existing local binary from deploy/prebuilt/eventlake"; \
        cp /tmp/prebuilt/eventlake /usr/local/bin/eventlake; \
    else \
        echo "==> [Prebuilt] Local binary not found. Downloading release binary from GitHub..."; \
        PROXY_PREFIX=""; \
        if [ "${USE_CN_PROXY}" = "true" ] || [ "${USE_CN_PROXY}" = "1" ]; then \
            PROXY_PREFIX="https://ghproxy.net/"; \
        fi; \
        ARCH="$(uname -m)"; \
        case "$ARCH" in \
            x86_64|amd64) ARCH_TAG="amd64" ;; \
            *) ARCH_TAG="amd64" ;; \
        esac; \
        if [ -n "${EVENTLAKE_DOWNLOAD_URL}" ]; then \
            DOWNLOAD_URL="${EVENTLAKE_DOWNLOAD_URL}"; \
        else \
            TAG="${EVENTLAKE_VERSION}"; \
            if [ "$TAG" = "latest" ]; then \
                API_URL="${PROXY_PREFIX}https://api.github.com/repos/${GITHUB_REPO}/releases/latest"; \
                TAG=$(curl -sSL "$API_URL" | grep -o '"tag_name": *"[^"]*"' | head -n 1 | cut -d '"' -f 4 || true); \
                if [ -z "$TAG" ]; then \
                    TAG=$(curl -sSL -I -o /dev/null -w "%{url_effective}" "https://github.com/${GITHUB_REPO}/releases/latest" | sed 's#.*/tag/##'); \
                fi; \
            fi; \
            if [ -z "$TAG" ] || [ "$TAG" = "latest" ]; then \
                echo "Error: unable to determine GitHub release tag for ${GITHUB_REPO}. Please specify EVENTLAKE_VERSION or build locally via scripts/build-prebuilt-binary.sh" >&2; \
                exit 1; \
            fi; \
            case "$TAG" in v*) ;; *) TAG="v${TAG}" ;; esac; \
            ARCHIVE_NAME="eventlake-${TAG}-linux-${ARCH_TAG}.tar.gz"; \
            DOWNLOAD_URL="${PROXY_PREFIX}https://github.com/${GITHUB_REPO}/releases/download/${TAG}/${ARCHIVE_NAME}"; \
        fi; \
        echo "==> [Prebuilt] Downloading binary from: ${DOWNLOAD_URL}"; \
        mkdir -p /tmp/dl; \
        curl -fSL "${DOWNLOAD_URL}" -o /tmp/dl/archive.tar.gz; \
        tar -xzf /tmp/dl/archive.tar.gz -C /tmp/dl; \
        if [ -f /tmp/dl/eventlake ]; then \
            cp /tmp/dl/eventlake /usr/local/bin/eventlake; \
        elif [ -f /tmp/dl/target/release/eventlake ]; then \
            cp /tmp/dl/target/release/eventlake /usr/local/bin/eventlake; \
        else \
            echo "Error: eventlake binary not found in release archive" >&2; \
            exit 1; \
        fi; \
        rm -rf /tmp/dl; \
    fi; \
    chmod 0755 /usr/local/bin/eventlake; \
    rm -rf /tmp/prebuilt; \
    /usr/local/bin/eventlake --version || true

USER eventlake:eventlake

# ====================================================
# 3. 目标阶段 B: source (本地 Rust 源码多阶段完整编译)
# 供本地深度二次开发或需要重新完整编译验证时使用
# ====================================================
FROM rust:${RUST_VERSION}-bookworm AS builder

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY clickhouse ./clickhouse
COPY migrations ./migrations
COPY src ./src

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    cargo build --release --locked && \
    cp /app/target/release/eventlake /usr/local/bin/eventlake

FROM runtime-base AS source

USER root
COPY --from=builder /usr/local/bin/eventlake /usr/local/bin/eventlake
USER eventlake:eventlake
