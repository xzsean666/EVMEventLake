# syntax=docker/dockerfile:1

ARG RUST_VERSION=1.94

# ====================================================
# 1. 基础运行时阶段 (runtime-base)
# 公共运行环境，基于 Debian slim，不含任何 Rust 编译工具链
# ====================================================
FROM debian:bookworm-slim AS runtime-base

RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates curl && \
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
# 2. 目标阶段 A: prebuilt (免编译秒级打包)
# 供 GitHub Action 与本地 docker-compose 快速部署使用：
# 直接将宿主机/构建上下文中已编译好的二进制拷贝进容器。
# BuildKit 会自动跳过 builder 阶段，不拉取 Rust 镜像。
# ====================================================
FROM runtime-base AS prebuilt

ARG EVENTLAKE_BINARY=deploy/prebuilt/eventlake
USER root
COPY ${EVENTLAKE_BINARY} /usr/local/bin/eventlake
RUN chmod 0755 /usr/local/bin/eventlake
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
