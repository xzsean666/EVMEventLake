# EventLake Docker Deployment

Version: 1.0

Status: Current implementation

## 1. Deployment Modes

EventLake supports three Docker deployment entrypoints:

| Mode | Files | When to use |
| --- | --- | --- |
| Source build | `Dockerfile`, `docker-compose.yml` | CI or hosts that can download Rust crates and Debian packages. |
| Prebuilt binary | `scripts/build-prebuilt-binary.sh`, `Dockerfile.prebuilt`, `docker-compose.prebuilt.yml` | Hosts that should only package and run an already-built Linux binary. |
| China prebuilt binary | `Dockerfile.prebuilt.cn`, `docker-compose.prebuilt.cn.yml` | China mainland hosts where Docker Hub, Debian apt, or Cargo access is unstable. |
| ClickHouse source build | `Dockerfile.clickhouse`, `docker-compose.clickhouse.yml` | Build EventLake with analytical search enabled. |
| ClickHouse prebuilt binary | `Dockerfile.prebuilt.clickhouse`, `docker-compose.prebuilt.clickhouse.yml` | Run the ClickHouse-enabled prebuilt binary. |
| ClickHouse China prebuilt | `Dockerfile.prebuilt.clickhouse.cn`, `docker-compose.prebuilt.clickhouse.cn.yml` | China-optimized ClickHouse prebuilt deployment. |

The original modes use:

- `postgres`
- `eventlake`

The ClickHouse variants add a `clickhouse` service for large analytical searches.
PostgreSQL remains the raw-log and operational source of truth; in this mode decoded
events and search indexes are stored only in ClickHouse.

## 2. Runtime Facts

- Rust package: `eventlake`
- Deployed binary: `eventlake`
- Container user: `eventlake`
- HTTP host env: `EVENTLAKE_HTTP_HOST`
- HTTP port env: `EVENTLAKE_HTTP_PORT`
- Default container port: `8080`
- Default host port: `EVENTLAKE_HTTP_PORT`, default `8080`
- Health endpoint: `/health/ready`
- Persistent state: PostgreSQL; ClickHouse variants also persist the selected derived-event search store

The application compiles migrations into the binary with `sqlx::migrate!("./migrations")`.
The source-build Dockerfile copies `migrations/` into the builder stage so the release
binary includes the migration set.

## 3. Environment File

Use `.env.example` as the template for local Docker deployment. For a real deployment,
create an environment file with the same keys and change at least:

- `EVENTLAKE_JWT_SECRET`
- `EVENTLAKE_DATABASE_URL`

When using a non-default env file, pass it in both places:

```bash
EVENTLAKE_ENV_FILE=.env docker compose --env-file .env up -d --build eventlake
```

`--env-file` feeds Compose interpolation. `EVENTLAKE_ENV_FILE` selects the env file
mounted into the `eventlake` container.

## 4. Source Build Deployment

```bash
docker compose --env-file .env.example up -d --build eventlake
docker compose --env-file .env.example ps
curl -fsS http://127.0.0.1:8080/health/ready
```

Stop services:

```bash
docker compose down
```

Remove the local PostgreSQL volume only for disposable local environments:

```bash
docker compose down -v
```

## 5. Prebuilt Binary Deployment

Build the Linux binary first:

```bash
scripts/build-prebuilt-binary.sh
```

Then build and run the lightweight runtime image:

```bash
docker compose --env-file .env.example -f docker-compose.prebuilt.yml up -d --build eventlake
docker compose --env-file .env.example -f docker-compose.prebuilt.yml ps
curl -fsS http://127.0.0.1:8080/health/ready
```

Optional variables:

| Variable | Purpose |
| --- | --- |
| `EVENTLAKE_PREBUILT_BINARY` | Override the generated binary path. Default: `deploy/prebuilt/eventlake`. |
| `EVENTLAKE_CARGO_TARGET` | Pass a target triple to `cargo build --target`. |
| `CARGO_TARGET_DIR` | Override Cargo's target directory. |

The prebuilt binary must match the runtime container OS, CPU architecture, and libc.
For the default Debian slim runtime, build a Linux glibc binary.

## 6. China Prebuilt Deployment

Build the binary before running Docker on the target host:

```bash
scripts/build-prebuilt-binary.sh
```

Run the China-optimized compose file:

```bash
docker compose --env-file .env.example -f docker-compose.prebuilt.cn.yml up -d --build eventlake
docker compose --env-file .env.example -f docker-compose.prebuilt.cn.yml ps
curl -fsS http://127.0.0.1:8080/health/ready
```

Overridable China network variables:

| Variable | Default |
| --- | --- |
| `EVENTLAKE_DEBIAN_IMAGE` | `m.daocloud.io/docker.io/library/debian:bookworm-slim` |
| `EVENTLAKE_DEBIAN_MIRROR` | `http://mirrors.aliyun.com/debian` |
| `EVENTLAKE_DEBIAN_SECURITY_MIRROR` | `http://mirrors.aliyun.com/debian-security` |
| `EVENTLAKE_POSTGRES_IMAGE` | `m.daocloud.io/docker.io/library/postgres:18` |

## 7. ClickHouse Deployment

Build the ClickHouse-enabled binary for a prebuilt variant:

```bash
EVENTLAKE_PREBUILT_BINARY=deploy/prebuilt/eventlake-clickhouse scripts/build-prebuilt-binary.sh
```

Run the source-build variant:

```bash
docker compose --env-file .env.example -f docker-compose.clickhouse.yml up -d --build
```

For prebuilt variants, replace the Compose file with
`docker-compose.prebuilt.clickhouse.yml` or `docker-compose.prebuilt.clickhouse.cn.yml`.
The ClickHouse Compose files set `EVENTLAKE_CLICKHOUSE_ENABLED=true` and connect to the
`clickhouse` service over HTTP port `8123`.

## 8. Verification

Static checks:

```bash
cargo build --release --locked
scripts/build-prebuilt-binary.sh
docker compose config
docker compose -f docker-compose.prebuilt.yml config
docker compose -f docker-compose.prebuilt.cn.yml config
docker compose -f docker-compose.clickhouse.yml config
docker compose -f docker-compose.prebuilt.clickhouse.yml config
docker compose -f docker-compose.prebuilt.clickhouse.cn.yml config
```

Image checks:

```bash
docker build -f Dockerfile -t eventlake:local .
docker build -f Dockerfile.prebuilt -t eventlake:prebuilt .
docker build -f Dockerfile.prebuilt.cn -t eventlake:prebuilt-cn .
docker build -f Dockerfile.clickhouse -t eventlake:clickhouse .
```

Runtime check:

```bash
curl -fsS http://127.0.0.1:8080/health/ready
```

## 9. Optional SSH Tunnel Proxy

The prebuilt Docker images include `scripts/docker-ssh-tunnel-proxy.sh` as the
container entrypoint. The Dockerfiles, compose files, and script default
`SSH_TUNNEL_ENABLED=false`, so the wrapper simply starts EventLake unless you
explicitly enable it. If tunnel variables are set and
`SSH_TUNNEL_ENABLED=true`, it opens an SSH dynamic SOCKS5 tunnel and exports:

- `ALL_PROXY`
- `HTTP_PROXY`
- `HTTPS_PROXY`
- lowercase equivalents
- `NO_PROXY`

This is a process-level proxy environment, not an iptables transparent proxy.
It covers EventLake's `reqwest` JSON-RPC calls and other clients that honor
`ALL_PROXY`/`HTTP_PROXY`/`HTTPS_PROXY`. Arbitrary raw TCP traffic would need a
separate privileged transparent-proxy setup.

Minimum `.env` values:

```bash
SSH_TUNNEL_ENABLED=true
SSH_TUNNEL_HOST=203.0.113.10
SSH_TUNNEL_PORT=22
SSH_TUNNEL_USER=root
SSH_TUNNEL_PRIVATE_KEY_B64=...
SSH_TUNNEL_NO_PROXY=127.0.0.1,localhost,::1,postgres,clickhouse,eventlake
```

Prefer `SSH_TUNNEL_PRIVATE_KEY_B64` because multiline private keys are fragile in
environment files:

```bash
base64 -w0 ~/.ssh/id_ed25519
```

The script is generic. To reuse it in another Docker image, install
`openssh-client`, copy the script into the image, and wrap the service command:

```dockerfile
COPY scripts/docker-ssh-tunnel-proxy.sh /usr/local/bin/docker-ssh-tunnel-proxy
RUN chmod 0755 /usr/local/bin/docker-ssh-tunnel-proxy
ENV SSH_TUNNEL_ENABLED=false
ENTRYPOINT ["docker-ssh-tunnel-proxy"]
CMD ["your-service"]
```
